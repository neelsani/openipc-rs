use std::{borrow::Cow, collections::HashMap, sync::Arc, sync::OnceLock};

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
    decode_cache: Arc<HashMap<u128, Box<[u8]>>>,
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

        let mut code = Self {
            k,
            n,
            enc_matrix,
            decode_cache: Arc::new(HashMap::new()),
        };
        code.decode_cache = Arc::new(code.precompute_decode_matrices()?);
        Ok(code)
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
        let mut parity_cursor = self.k;

        for primary_idx in 0..self.k {
            if let Some(fragment) = fragments[primary_idx].as_ref() {
                if fragment.len() < block_size {
                    return Err(FecError::InvalidParameters);
                }
                indexes.push(primary_idx);
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
                    let input = fragments[indexes[col]]
                        .as_deref()
                        .expect("selected fragment exists");
                    addmul(&mut out, input, dec_matrix[row * self.k + col], block_size);
                }
                fragments[row] = Some(out);
                recovered += 1;
            }
        }

        Ok(recovered)
    }

    /// Recover missing primary fragments into caller-owned reusable buffers.
    ///
    /// `present` identifies received fragments. `fragments` contains exactly
    /// `n * block_size` contiguous bytes in fragment-index order. This avoids
    /// allocating recovered fragments in the packet-processing hot path.
    pub fn recover_primary_into(
        &self,
        fragments: &mut [u8],
        present: &mut [bool],
        block_size: usize,
    ) -> Result<usize, FecError> {
        if fragments.len() != self.n * block_size || present.len() != self.n {
            return Err(FecError::InvalidParameters);
        }
        if present[..self.k].iter().all(|is_present| *is_present) {
            return Ok(0);
        }

        // WFB uses k=8. Keep its per-damaged-block index selection on the
        // stack while preserving support for larger custom FEC codes.
        let mut stack_indexes = [0usize; 16];
        let mut heap_indexes = if self.k > stack_indexes.len() {
            vec![0; self.k]
        } else {
            Vec::new()
        };
        let indexes = if self.k <= stack_indexes.len() {
            &mut stack_indexes[..self.k]
        } else {
            heap_indexes.as_mut_slice()
        };
        let mut parity_cursor = self.k;
        for primary_idx in 0..self.k {
            if present[primary_idx] {
                indexes[primary_idx] = primary_idx;
            } else {
                while parity_cursor < self.n && !present[parity_cursor] {
                    parity_cursor += 1;
                }
                if parity_cursor >= self.n {
                    return Err(FecError::NotEnoughFragments);
                }
                indexes[primary_idx] = parity_cursor;
                parity_cursor += 1;
            }
        }

        self.validate_indexes(indexes)?;
        let dec_matrix = self.decode_matrix(indexes)?;
        let mut recovered = 0;
        for row in 0..self.k {
            if indexes[row] < self.k {
                continue;
            }

            fragment_mut(fragments, row, block_size).fill(0);
            for col in 0..self.k {
                addmul_distinct_contiguous(
                    fragments,
                    row,
                    indexes[col],
                    dec_matrix[row * self.k + col],
                    block_size,
                );
            }
            present[row] = true;
            recovered += 1;
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

    fn decode_matrix(&self, indexes: &[usize]) -> Result<Cow<'_, [u8]>, FecError> {
        if let Some(key) = index_key(indexes) {
            if let Some(matrix) = self.decode_cache.get(&key) {
                return Ok(Cow::Borrowed(matrix));
            }
        }
        Ok(Cow::Owned(self.decode_matrix_uncached(indexes)?))
    }

    fn decode_matrix_uncached(&self, indexes: &[usize]) -> Result<Vec<u8>, FecError> {
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

    fn precompute_decode_matrices(&self) -> Result<HashMap<u128, Box<[u8]>>, FecError> {
        const MAX_CACHED_MATRICES: usize = 4_096;

        // Exhaustively caching larger codes can consume substantial memory and
        // delay session setup. WFB's normal 8/12 code has only 494 recoverable
        // primary/parity loss patterns and fits comfortably within this bound.
        let parity_count = self.n - self.k;
        if self.k > 12
            || parity_count > 12
            || self.k >= usize::BITS as usize
            || parity_count >= usize::BITS as usize
        {
            return Ok(HashMap::new());
        }

        let pattern_count = (1..=self.k.min(parity_count)).fold(0usize, |total, missing| {
            total.saturating_add(
                binomial(self.k, missing).saturating_mul(binomial(parity_count, missing)),
            )
        });
        if pattern_count > MAX_CACHED_MATRICES {
            return Ok(HashMap::new());
        }

        let mut cache = HashMap::with_capacity(pattern_count);
        for primary_mask in 1usize..(1usize << self.k) {
            let missing = primary_mask.count_ones() as usize;
            if missing > parity_count {
                continue;
            }

            for parity_mask in 1usize..(1usize << parity_count) {
                if parity_mask.count_ones() as usize != missing {
                    continue;
                }

                let mut selected_parity = (0..parity_count)
                    .filter(|parity| parity_mask & (1usize << parity) != 0)
                    .map(|parity| self.k + parity);
                let indexes: Vec<usize> = (0..self.k)
                    .map(|primary| {
                        if primary_mask & (1usize << primary) == 0 {
                            primary
                        } else {
                            selected_parity.next().expect("matching parity count")
                        }
                    })
                    .collect();
                let matrix = self.decode_matrix_uncached(&indexes)?.into_boxed_slice();
                cache.insert(
                    index_key(&indexes).expect("cached FEC indexes fit u128"),
                    matrix,
                );
            }
        }
        Ok(cache)
    }
}

fn binomial(n: usize, k: usize) -> usize {
    let k = k.min(n - k);
    (0..k).fold(1usize, |result, index| result * (n - index) / (index + 1))
}

fn index_key(indexes: &[usize]) -> Option<u128> {
    if indexes.len() > 16 || indexes.iter().any(|&index| index > u8::MAX as usize) {
        return None;
    }

    Some(
        indexes
            .iter()
            .enumerate()
            .fold(0u128, |key, (offset, &index)| {
                key | (index as u128) << (offset * 8)
            }),
    )
}

#[derive(Clone)]
struct GfTables {
    gf_exp: [u8; 510],
    inverse: [u8; 256],
    gf_mul: Box<[[u8; 256]; 256]>,
    gf_mul_low: Box<[[u8; 16]; 256]>,
    gf_mul_high: Box<[[u8; 16]; 256]>,
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

        let mut gf_mul_low = Box::new([[0; 16]; 256]);
        let mut gf_mul_high = Box::new([[0; 16]; 256]);
        for coefficient in 0..256 {
            for nibble in 0..16 {
                gf_mul_low[coefficient][nibble] = gf_mul[coefficient][nibble];
                gf_mul_high[coefficient][nibble] = gf_mul[coefficient][nibble << 4];
            }
        }

        Self {
            gf_exp,
            inverse,
            gf_mul,
            gf_mul_low,
            gf_mul_high,
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

#[inline(always)]
fn addmul(dst: &mut [u8], src: &[u8], coefficient: u8, len: usize) {
    if coefficient == 0 {
        return;
    }
    if coefficient == 1 {
        for (output, input) in dst[..len].iter_mut().zip(&src[..len]) {
            *output ^= *input;
        }
        return;
    }
    let tables = tables();
    let coefficient = coefficient as usize;
    let vector_len = crate::fec_simd::addmul(
        &mut dst[..len],
        &src[..len],
        &tables.gf_mul_low[coefficient],
        &tables.gf_mul_high[coefficient],
    );
    let mul = &tables.gf_mul[coefficient];
    for idx in vector_len..len {
        dst[idx] ^= mul[src[idx] as usize];
    }
}

fn fragment_mut(data: &mut [u8], index: usize, block_size: usize) -> &mut [u8] {
    &mut data[index * block_size..(index + 1) * block_size]
}

fn addmul_distinct_contiguous(
    fragments: &mut [u8],
    dst_idx: usize,
    src_idx: usize,
    coefficient: u8,
    len: usize,
) {
    debug_assert_ne!(dst_idx, src_idx);
    if dst_idx < src_idx {
        let (before_src, from_src) = fragments.split_at_mut(src_idx * len);
        let dst = &mut before_src[dst_idx * len..(dst_idx + 1) * len];
        addmul(dst, &from_src[..len], coefficient, len);
    } else {
        let (before_dst, from_dst) = fragments.split_at_mut(dst_idx * len);
        let src = &before_dst[src_idx * len..(src_idx + 1) * len];
        addmul(&mut from_dst[..len], src, coefficient, len);
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

    #[test]
    fn optimized_addmul_matches_scalar_galois_field_math() {
        let src: Vec<u8> = (0..79).map(|idx| (idx * 37 + 11) as u8).collect();
        let initial: Vec<u8> = (0..79).map(|idx| (idx * 19 + 7) as u8).collect();

        for coefficient in 0..=u8::MAX {
            let mut actual = initial.clone();
            addmul(&mut actual, &src, coefficient, src.len());

            let mut expected = initial.clone();
            for (output, input) in expected.iter_mut().zip(&src) {
                *output ^= gf_mul(coefficient, *input);
            }
            assert_eq!(actual, expected, "coefficient {coefficient}");
        }
    }

    #[test]
    fn recovers_every_supported_wfb_eight_twelve_loss_pattern() {
        let fec = FecCode::new(8, 12).unwrap();
        assert_eq!(fec.decode_cache.len(), 494);

        let primary: Vec<Vec<u8>> = (0..fec.k())
            .map(|fragment| {
                (0..257)
                    .map(|offset| (fragment * 31 + offset * 17) as u8)
                    .collect()
            })
            .collect();
        let parity = fec.encode(&primary, 257).unwrap();
        let complete: Vec<Vec<u8>> = primary.iter().chain(&parity).cloned().collect();

        for mask in 1u16..(1 << fec.k()) {
            let missing = mask.count_ones() as usize;
            if missing > fec.n() - fec.k() {
                continue;
            }

            let mut fragments: Vec<Option<Vec<u8>>> = complete.iter().cloned().map(Some).collect();
            for (primary_idx, fragment) in fragments.iter_mut().enumerate().take(fec.k()) {
                if mask & (1 << primary_idx) != 0 {
                    *fragment = None;
                }
            }

            assert_eq!(fec.recover_primary(&mut fragments, 257), Ok(missing));
            for (fragment, expected) in fragments.iter().zip(&primary) {
                assert_eq!(fragment.as_deref(), Some(&expected[..]));
            }

            let mut contiguous: Vec<u8> = complete.iter().flatten().copied().collect();
            let mut present = vec![true; fec.n()];
            for (primary_idx, is_present) in present.iter_mut().enumerate().take(fec.k()) {
                if mask & (1 << primary_idx) != 0 {
                    *is_present = false;
                }
            }
            assert_eq!(
                fec.recover_primary_into(&mut contiguous, &mut present, 257),
                Ok(missing)
            );
            for (primary_idx, expected) in primary.iter().enumerate() {
                let start = primary_idx * 257;
                assert_eq!(&contiguous[start..start + 257], expected);
            }
        }
    }

    #[test]
    fn cached_recovery_handles_every_primary_and_parity_loss_pattern() {
        let fec = FecCode::new(8, 12).unwrap();
        let primary: Vec<Vec<u8>> = (0..fec.k())
            .map(|fragment| {
                (0..257)
                    .map(|offset| (fragment * 43 + offset * 29) as u8)
                    .collect()
            })
            .collect();
        let parity = fec.encode(&primary, 257).unwrap();
        let complete: Vec<Vec<u8>> = primary.iter().chain(&parity).cloned().collect();
        let parity_count = fec.n() - fec.k();
        for primary_mask in 1u16..(1 << fec.k()) {
            let missing = primary_mask.count_ones() as usize;
            if missing > parity_count {
                continue;
            }
            for parity_mask in 1u16..(1 << parity_count) {
                if parity_mask.count_ones() as usize != missing {
                    continue;
                }

                let mut fragments: Vec<Option<Vec<u8>>> =
                    complete.iter().cloned().map(Some).collect();
                for (primary_idx, fragment) in fragments.iter_mut().enumerate().take(fec.k()) {
                    if primary_mask & (1 << primary_idx) != 0 {
                        *fragment = None;
                    }
                }
                for parity_idx in 0..parity_count {
                    if parity_mask & (1 << parity_idx) == 0 {
                        fragments[fec.k() + parity_idx] = None;
                    }
                }

                assert_eq!(fec.recover_primary(&mut fragments, 257), Ok(missing));
                for (fragment, expected) in fragments.iter().zip(&primary) {
                    assert_eq!(fragment.as_deref(), Some(&expected[..]));
                }
            }
        }
    }
}
