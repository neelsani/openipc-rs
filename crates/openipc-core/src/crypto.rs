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

    let mut mac_data = Vec::with_capacity(
        aad.len() + pad16_len(aad.len()) + ciphertext.len() + pad16_len(ciphertext.len()) + 16,
    );
    mac_data.extend_from_slice(aad);
    mac_data.extend(std::iter::repeat_n(0, pad16_len(aad.len())));
    mac_data.extend_from_slice(ciphertext);
    mac_data.extend(std::iter::repeat_n(0, pad16_len(ciphertext.len())));
    mac_data.extend_from_slice(&(aad.len() as u64).to_le_bytes());
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

const fn pad16_len(len: usize) -> usize {
    (16 - (len % 16)) % 16
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
}
