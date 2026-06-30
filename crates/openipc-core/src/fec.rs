use std::sync::OnceLock;

const GF_SIZE: usize = 255;
const GF_BITS: usize = 8;
const PRIMITIVE_POLY: &[u8; 9] = b"101110001";

static GF_TABLES: OnceLock<GfTables> = OnceLock::new();

/// Reed-Solomon FEC error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FecError {
    /// FEC parameters are outside `0 < k <= n < 256`.
    InvalidParameters,
    /// Fewer than `k` usable fragments were available.
    NotEnoughFragments,
    /// A fragment index was outside the configured block.
    InvalidFragmentIndex(usize),
    /// The decode matrix could not be inverted.
    SingularMatrix,
    /// Recovered output did not match expected primary slots.
    OutputSlotMismatch,
}

impl std::fmt::Display for FecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidParameters => write!(f, "invalid FEC parameters"),
            Self::NotEnoughFragments => write!(f, "not enough fragments to recover block"),
            Self::InvalidFragmentIndex(idx) => write!(f, "invalid FEC fragment index {idx}"),
            Self::SingularMatrix => write!(f, "FEC decode matrix is singular"),
            Self::OutputSlotMismatch => write!(f, "FEC output slot mismatch"),
        }
    }
}

impl std::error::Error for FecError {}

/// Reed-Solomon FEC code used by WFB blocks.
#[derive(Debug, Clone)]
pub struct FecCode {
    k: usize,
    n: usize,
    enc_matrix: Vec<u8>,
}

impl FecCode {
    /// Create an FEC code with `k` primary fragments and `n-k` parity fragments.
    pub fn new(k: usize, n: usize) -> Result<Self, FecError> {
        if k == 0 || n == 0 || k > n || n >= 256 {
            return Err(FecError::InvalidParameters);
        }

        let tables = tables();
        let mut tmp = vec![0; n * k];
        tmp[0] = 1;
        for row in 0..(n - 1) {
            for col in 0..k {
                tmp[(row + 1) * k + col] = tables.gf_exp[modnn((row * col) as i32) as usize];
            }
        }

        invert_vdm(&mut tmp[..k * k], k)?;

        let mut enc_matrix = vec![0; n * k];
        if n > k {
            matmul(
                &tmp[k * k..],
                &tmp[..k * k],
                &mut enc_matrix[k * k..],
                n - k,
                k,
                k,
            );
        }
        for col in 0..k {
            enc_matrix[col * k + col] = 1;
        }

        Ok(Self { k, n, enc_matrix })
    }

    /// Return the number of primary fragments.
    pub const fn k(&self) -> usize {
        self.k
    }

    /// Return the total number of primary plus parity fragments.
    pub const fn n(&self) -> usize {
        self.n
    }

    /// Generate parity fragments for a full primary block.
    pub fn encode(&self, primary: &[Vec<u8>], block_size: usize) -> Result<Vec<Vec<u8>>, FecError> {
        if primary.len() != self.k || primary.iter().any(|fragment| fragment.len() < block_size) {
            return Err(FecError::InvalidParameters);
        }

        let mut fecs = vec![vec![0; block_size]; self.n - self.k];
        for (fec_offset, fec) in fecs.iter_mut().enumerate() {
            let fecnum = self.k + fec_offset;
            let matrix_row = &self.enc_matrix[fecnum * self.k..(fecnum + 1) * self.k];
            for (src_idx, src) in primary.iter().enumerate() {
                addmul(fec, src, matrix_row[src_idx], block_size);
            }
        }
        Ok(fecs)
    }

    /// Recover missing primary fragments in-place.
    pub fn recover_primary(
        &self,
        fragments: &mut [Option<Vec<u8>>],
        block_size: usize,
    ) -> Result<usize, FecError> {
        if fragments.len() != self.n {
            return Err(FecError::InvalidParameters);
        }
        if (0..self.k).all(|idx| fragments[idx].is_some()) {
            return Ok(0);
        }

        let mut indexes = Vec::with_capacity(self.k);
        let mut inputs = Vec::with_capacity(self.k);
        let mut parity_cursor = self.k;

        for primary_idx in 0..self.k {
            if let Some(fragment) = fragments[primary_idx].as_ref() {
                if fragment.len() < block_size {
                    return Err(FecError::InvalidParameters);
                }
                indexes.push(primary_idx);
                inputs.push(fragment.clone());
            } else {
                while parity_cursor < self.n && fragments[parity_cursor].is_none() {
                    parity_cursor += 1;
                }
                if parity_cursor >= self.n {
                    return Err(FecError::NotEnoughFragments);
                }
                let fragment = fragments[parity_cursor]
                    .as_ref()
                    .ok_or(FecError::NotEnoughFragments)?;
                if fragment.len() < block_size {
                    return Err(FecError::InvalidParameters);
                }
                indexes.push(parity_cursor);
                inputs.push(fragment.clone());
                parity_cursor += 1;
            }
        }

        self.validate_indexes(&indexes)?;
        let dec_matrix = self.decode_matrix(&indexes)?;
        let mut recovered = 0usize;

        for row in 0..self.k {
            if indexes[row] >= self.k {
                let mut out = vec![0; block_size];
                for col in 0..self.k {
                    addmul(
                        &mut out,
                        &inputs[col],
                        dec_matrix[row * self.k + col],
                        block_size,
                    );
                }
                fragments[row] = Some(out);
                recovered += 1;
            }
        }

        Ok(recovered)
    }

    fn validate_indexes(&self, indexes: &[usize]) -> Result<(), FecError> {
        if indexes.len() != self.k {
            return Err(FecError::NotEnoughFragments);
        }
        for (row, &idx) in indexes.iter().enumerate() {
            if idx >= self.n {
                return Err(FecError::InvalidFragmentIndex(idx));
            }
            if idx < self.k && idx != row {
                return Err(FecError::OutputSlotMismatch);
            }
        }
        Ok(())
    }

    fn decode_matrix(&self, indexes: &[usize]) -> Result<Vec<u8>, FecError> {
        let mut matrix = vec![0; self.k * self.k];
        for (row, &idx) in indexes.iter().enumerate() {
            let row_start = row * self.k;
            if idx < self.k {
                matrix[row_start + row] = 1;
            } else {
                matrix[row_start..row_start + self.k]
                    .copy_from_slice(&self.enc_matrix[idx * self.k..(idx + 1) * self.k]);
            }
        }
        invert_mat(&mut matrix, self.k)?;
        Ok(matrix)
    }
}

#[derive(Clone)]
struct GfTables {
    gf_exp: [u8; 510],
    inverse: [u8; 256],
    gf_mul: Box<[[u8; 256]; 256]>,
}

fn tables() -> &'static GfTables {
    GF_TABLES.get_or_init(GfTables::new)
}

impl GfTables {
    fn new() -> Self {
        let mut gf_exp = [0; 510];
        let mut gf_log = [0; 256];
        let mut inverse = [0; 256];

        let mut mask = 1u8;
        gf_exp[GF_BITS] = 0;
        for i in 0..GF_BITS {
            gf_exp[i] = mask;
            gf_log[mask as usize] = i as u16;
            if PRIMITIVE_POLY[i] == b'1' {
                gf_exp[GF_BITS] ^= mask;
            }
            mask <<= 1;
        }
        gf_log[gf_exp[GF_BITS] as usize] = GF_BITS as u16;

        mask = 1 << (GF_BITS - 1);
        for i in (GF_BITS + 1)..GF_SIZE {
            gf_exp[i] = if gf_exp[i - 1] >= mask {
                gf_exp[GF_BITS] ^ ((gf_exp[i - 1] ^ mask) << 1)
            } else {
                gf_exp[i - 1] << 1
            };
            gf_log[gf_exp[i] as usize] = i as u16;
        }
        gf_log[0] = GF_SIZE as u16;
        for i in 0..GF_SIZE {
            gf_exp[i + GF_SIZE] = gf_exp[i];
        }

        inverse[1] = 1;
        for i in 2..=GF_SIZE {
            inverse[i] = gf_exp[GF_SIZE - gf_log[i] as usize];
        }

        let mut gf_mul = Box::new([[0; 256]; 256]);
        for i in 1..256 {
            for j in 1..256 {
                gf_mul[i][j] = gf_exp[modnn(gf_log[i] as i32 + gf_log[j] as i32) as usize];
            }
        }

        Self {
            gf_exp,
            inverse,
            gf_mul,
        }
    }
}

fn modnn(mut x: i32) -> u8 {
    while x >= GF_SIZE as i32 {
        x -= GF_SIZE as i32;
        x = (x >> GF_BITS) + (x & GF_SIZE as i32);
    }
    x as u8
}

fn gf_mul(x: u8, y: u8) -> u8 {
    tables().gf_mul[x as usize][y as usize]
}

fn addmul(dst: &mut [u8], src: &[u8], coefficient: u8, len: usize) {
    if coefficient == 0 {
        return;
    }
    let mul = &tables().gf_mul[coefficient as usize];
    for idx in 0..len {
        dst[idx] ^= mul[src[idx] as usize];
    }
}

fn matmul(a: &[u8], b: &[u8], c: &mut [u8], n: usize, k: usize, m: usize) {
    for row in 0..n {
        for col in 0..m {
            let mut acc = 0;
            for i in 0..k {
                acc ^= gf_mul(a[row * k + i], b[i * m + col]);
            }
            c[row * m + col] = acc;
        }
    }
}

fn invert_mat(src: &mut [u8], k: usize) -> Result<(), FecError> {
    let mut indxc = vec![0; k];
    let mut indxr = vec![0; k];
    let mut ipiv = vec![0; k];
    let mut id_row = vec![0; k];

    for col in 0..k {
        let mut irow = None;
        let mut icol = None;

        if ipiv[col] != 1 && src[col * k + col] != 0 {
            irow = Some(col);
            icol = Some(col);
        } else {
            'search: for row in 0..k {
                if ipiv[row] != 1 {
                    for ix in 0..k {
                        if ipiv[ix] == 0 && src[row * k + ix] != 0 {
                            irow = Some(row);
                            icol = Some(ix);
                            break 'search;
                        }
                    }
                }
            }
        }

        let irow = irow.ok_or(FecError::SingularMatrix)?;
        let icol = icol.ok_or(FecError::SingularMatrix)?;
        ipiv[icol] += 1;

        if irow != icol {
            for ix in 0..k {
                src.swap(irow * k + ix, icol * k + ix);
            }
        }
        indxr[col] = irow;
        indxc[col] = icol;

        let pivot = src[icol * k + icol];
        if pivot == 0 {
            return Err(FecError::SingularMatrix);
        }
        if pivot != 1 {
            let inv = tables().inverse[pivot as usize];
            src[icol * k + icol] = 1;
            for ix in 0..k {
                src[icol * k + ix] = gf_mul(inv, src[icol * k + ix]);
            }
        }

        id_row[icol] = 1;
        if src[icol * k..(icol + 1) * k] != id_row[..] {
            let pivot_row = src[icol * k..(icol + 1) * k].to_vec();
            for ix in 0..k {
                if ix != icol {
                    let coefficient = src[ix * k + icol];
                    src[ix * k + icol] = 0;
                    addmul(&mut src[ix * k..(ix + 1) * k], &pivot_row, coefficient, k);
                }
            }
        }
        id_row[icol] = 0;
    }

    for col in (0..k).rev() {
        if indxr[col] != indxc[col] {
            for row in 0..k {
                src.swap(row * k + indxr[col], row * k + indxc[col]);
            }
        }
    }
    Ok(())
}

fn invert_vdm(src: &mut [u8], k: usize) -> Result<(), FecError> {
    if k == 1 {
        return Ok(());
    }

    let mut c = vec![0; k];
    let mut b = vec![0; k];
    let mut p = vec![0; k];

    for i in 0..k {
        p[i] = src[i * k + 1];
    }

    c[k - 1] = p[0];
    for (i, p_i) in p.iter().copied().enumerate().take(k).skip(1) {
        let start = k - 1 - (i - 1);
        for j in start..(k - 1) {
            c[j] ^= gf_mul(p_i, c[j + 1]);
        }
        c[k - 1] ^= p_i;
    }

    for row in 0..k {
        let xx = p[row];
        let mut t = 1;
        b[k - 1] = 1;
        for i in (1..k).rev() {
            b[i - 1] = c[i] ^ gf_mul(xx, b[i]);
            t = gf_mul(xx, t) ^ b[i - 1];
        }
        if t == 0 {
            return Err(FecError::SingularMatrix);
        }
        let inv = tables().inverse[t as usize];
        for col in 0..k {
            src[col * k + row] = gf_mul(inv, b[col]);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_missing_primary_fragment_from_parity() {
        let fec = FecCode::new(3, 5).unwrap();
        let primary = vec![b"aaaa".to_vec(), b"bbbb".to_vec(), b"cccc".to_vec()];
        let parity = fec.encode(&primary, 4).unwrap();
        let mut fragments = vec![
            Some(primary[0].clone()),
            None,
            Some(primary[2].clone()),
            Some(parity[0].clone()),
            None,
        ];

        let recovered = fec.recover_primary(&mut fragments, 4).unwrap();
        assert_eq!(recovered, 1);
        assert_eq!(fragments[1].as_deref(), Some(&primary[1][..]));
    }
}
