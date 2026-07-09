use chacha20::cipher::{KeyIvInit, StreamCipher, StreamCipherSeek};
use chacha20::ChaCha20Legacy;
use poly1305::universal_hash::KeyInit;
use poly1305::Poly1305;
use subtle::ConstantTimeEq;

/// Key length used by the legacy WFB ChaCha20-Poly1305 construction.
pub const CHACHA20_POLY1305_KEY_LEN: usize = 32;
/// Nonce length used by the legacy WFB ChaCha20-Poly1305 construction.
pub const CHACHA20_POLY1305_NONCE_LEN: usize = 8;
/// Authentication tag length used by the legacy WFB construction.
pub const CHACHA20_POLY1305_TAG_LEN: usize = 16;

/// Error from legacy WFB ChaCha20-Poly1305 helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoError {
    /// Key slice was not 32 bytes.
    InvalidKey,
    /// Nonce slice was not 8 bytes.
    InvalidNonce,
    /// Ciphertext did not include a full authentication tag.
    CiphertextTooShort,
    /// Authentication tag did not verify.
    AuthenticationFailed,
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKey => write!(f, "invalid key"),
            Self::InvalidNonce => write!(f, "invalid nonce"),
            Self::CiphertextTooShort => write!(f, "ciphertext is shorter than authentication tag"),
            Self::AuthenticationFailed => write!(f, "authentication failed"),
        }
    }
}

impl std::error::Error for CryptoError {}

/// Verify and decrypt the legacy WFB ChaCha20-Poly1305 payload shape.
pub fn decrypt_chacha20poly1305_legacy(
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    ciphertext_and_tag: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let mut plaintext = Vec::with_capacity(ciphertext_and_tag.len());
    decrypt_chacha20poly1305_legacy_into(key, nonce, aad, ciphertext_and_tag, &mut plaintext)?;
    Ok(plaintext)
}

/// Verify and decrypt into a reusable caller-owned buffer.
///
/// The legacy WFB packet format is fixed-size and is received at high packet
/// rates. Reusing this buffer avoids one allocation for every authenticated
/// data fragment while preserving the allocating convenience API above.
pub fn decrypt_chacha20poly1305_legacy_into(
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    ciphertext_and_tag: &[u8],
    plaintext: &mut Vec<u8>,
) -> Result<(), CryptoError> {
    if ciphertext_and_tag.len() < CHACHA20_POLY1305_TAG_LEN {
        return Err(CryptoError::CiphertextTooShort);
    }
    let ciphertext_len = ciphertext_and_tag.len() - CHACHA20_POLY1305_TAG_LEN;
    let ciphertext = &ciphertext_and_tag[..ciphertext_len];
    let expected_tag = &ciphertext_and_tag[ciphertext_len..];
    let tag = chacha20poly1305_legacy_tag(key, nonce, aad, ciphertext)?;
    if tag.ct_eq(expected_tag).unwrap_u8() != 1 {
        return Err(CryptoError::AuthenticationFailed);
    }

    plaintext.clear();
    plaintext.extend_from_slice(ciphertext);
    apply_chacha20_legacy_keystream(key, nonce, 64, plaintext.as_mut_slice())?;
    Ok(())
}

/// Encrypt and authenticate using the legacy WFB ChaCha20-Poly1305 shape.
pub fn encrypt_chacha20poly1305_legacy(
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let mut ciphertext = plaintext.to_vec();
    apply_chacha20_legacy_keystream(key, nonce, 64, &mut ciphertext)?;
    let tag = chacha20poly1305_legacy_tag(key, nonce, aad, &ciphertext)?;
    ciphertext.extend_from_slice(&tag);
    Ok(ciphertext)
}

fn chacha20poly1305_legacy_tag(
    key: &[u8],
    nonce: &[u8],
    aad: &[u8],
    ciphertext: &[u8],
) -> Result<[u8; CHACHA20_POLY1305_TAG_LEN], CryptoError> {
    let mut block0 = [0; 64];
    apply_chacha20_legacy_keystream(key, nonce, 0, &mut block0)?;
    let poly_key: [u8; 32] = block0[..32]
        .try_into()
        .map_err(|_| CryptoError::InvalidKey)?;

    // WFB uses libsodium's original 64-bit-nonce construction, which predates
    // the IETF layout. Its lengths immediately follow their respective byte
    // strings and neither section is padded to a 16-byte boundary.
    let mut mac_data = Vec::with_capacity(aad.len() + ciphertext.len() + 16);
    mac_data.extend_from_slice(aad);
    mac_data.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    mac_data.extend_from_slice(ciphertext);
    mac_data.extend_from_slice(&(ciphertext.len() as u64).to_le_bytes());

    let tag = Poly1305::new((&poly_key).into()).compute_unpadded(&mac_data);
    let mut out = [0; CHACHA20_POLY1305_TAG_LEN];
    out.copy_from_slice(&tag);
    Ok(out)
}

fn apply_chacha20_legacy_keystream(
    key: &[u8],
    nonce: &[u8],
    offset: u32,
    data: &mut [u8],
) -> Result<(), CryptoError> {
    let key: [u8; CHACHA20_POLY1305_KEY_LEN] =
        key.try_into().map_err(|_| CryptoError::InvalidKey)?;
    let nonce: [u8; CHACHA20_POLY1305_NONCE_LEN] =
        nonce.try_into().map_err(|_| CryptoError::InvalidNonce)?;
    let mut cipher = ChaCha20Legacy::new(&key.into(), &nonce.into());
    cipher.seek(offset);
    cipher.apply_keystream(data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_aead_roundtrips_with_aad() {
        let key = [7; 32];
        let nonce = [9; 8];
        let aad = b"wfb block header";
        let plaintext = b"rtp payload bytes";

        let encrypted = encrypt_chacha20poly1305_legacy(&key, &nonce, aad, plaintext).unwrap();
        assert_ne!(&encrypted[..plaintext.len()], plaintext);
        let decrypted = decrypt_chacha20poly1305_legacy(&key, &nonce, aad, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn legacy_aead_rejects_modified_tag() {
        let key = [7; 32];
        let nonce = [9; 8];
        let mut encrypted =
            encrypt_chacha20poly1305_legacy(&key, &nonce, b"aad", b"payload").unwrap();
        let last = encrypted.len() - 1;
        encrypted[last] ^= 0x80;

        assert_eq!(
            decrypt_chacha20poly1305_legacy(&key, &nonce, b"aad", &encrypted).unwrap_err(),
            CryptoError::AuthenticationFailed
        );
    }

    #[test]
    fn matches_libsodium_original_construction_vector() {
        // libsodium test/default/aead_chacha20poly1305.{c,exp}. This guards
        // against accidentally using the padded IETF Poly1305 input layout.
        let key = [
            0x42, 0x90, 0xbc, 0xb1, 0x54, 0x17, 0x35, 0x31, 0xf3, 0x14, 0xaf, 0x57, 0xf3, 0xbe,
            0x3b, 0x50, 0x06, 0xda, 0x37, 0x1e, 0xce, 0x27, 0x2a, 0xfa, 0x1b, 0x5d, 0xbd, 0xd1,
            0x10, 0x0a, 0x10, 0x07,
        ];
        let nonce = [0xcd, 0x7c, 0xf6, 0x7b, 0xe3, 0x9c, 0x79, 0x4a];
        let aad = [0x87, 0xe2, 0x29, 0xd4, 0x50, 0x08, 0x45, 0xa0, 0x79, 0xc0];
        let plaintext = [0x86, 0xd0, 0x99, 0x74, 0x84, 0x0b, 0xde, 0xd2, 0xa5, 0xca];
        let expected = [
            0xe3, 0xe4, 0x46, 0xf7, 0xed, 0xe9, 0xa1, 0x9b, 0x62, 0xa4, 0x67, 0x7d, 0xab, 0xf4,
            0xe3, 0xd2, 0x4b, 0x87, 0x6b, 0xb2, 0x84, 0x75, 0x38, 0x96, 0xe1, 0xd6,
        ];

        let encrypted = encrypt_chacha20poly1305_legacy(&key, &nonce, &aad, &plaintext).unwrap();
        assert_eq!(encrypted, expected);
        assert_eq!(
            decrypt_chacha20poly1305_legacy(&key, &nonce, &aad, &expected).unwrap(),
            plaintext
        );
    }
}
