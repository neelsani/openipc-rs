//! Architecture-specific Galois-field multiply/XOR loops.
//!
//! Callers provide two 16-byte lookup tables containing the products for the
//! low and high nibble of each source byte. Bounds checks and CPU feature
//! selection stay in safe Rust; only the vector loads and stores are unsafe.

const LANES: usize = 16;

#[inline(always)]
pub(crate) fn addmul(dst: &mut [u8], src: &[u8], low: &[u8; LANES], high: &[u8; LANES]) -> usize {
    let len = dst.len().min(src.len());

    #[cfg(target_arch = "aarch64")]
    let processed = {
        // NEON is part of the AArch64 baseline.
        unsafe { aarch64::addmul(&mut dst[..len], &src[..len], low, high) }
    };

    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    let processed = {
        if std::arch::is_x86_feature_detected!("ssse3") {
            unsafe { x86::addmul(&mut dst[..len], &src[..len], low, high) }
        } else {
            0
        }
    };

    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    let processed = unsafe { wasm::addmul(&mut dst[..len], &src[..len], low, high) };

    #[cfg(not(any(
        target_arch = "aarch64",
        target_arch = "x86",
        target_arch = "x86_64",
        all(target_arch = "wasm32", target_feature = "simd128")
    )))]
    let processed = 0;

    processed
}

#[cfg(target_arch = "aarch64")]
mod aarch64 {
    use core::arch::aarch64::*;

    pub(super) unsafe fn addmul(
        dst: &mut [u8],
        src: &[u8],
        low: &[u8; 16],
        high: &[u8; 16],
    ) -> usize {
        let vector_len = dst.len() & !15;
        let unrolled_len = dst.len() & !63;
        let low_table = vld1q_u8(low.as_ptr());
        let high_table = vld1q_u8(high.as_ptr());
        let nibble_mask = vdupq_n_u8(0x0f);

        for base in (0..unrolled_len).step_by(64) {
            for offset in [base, base + 16, base + 32, base + 48] {
                let input = vld1q_u8(src.as_ptr().add(offset));
                let low_product = vqtbl1q_u8(low_table, vandq_u8(input, nibble_mask));
                let high_product = vqtbl1q_u8(high_table, vshrq_n_u8(input, 4));
                let output = vld1q_u8(dst.as_ptr().add(offset));
                vst1q_u8(
                    dst.as_mut_ptr().add(offset),
                    veorq_u8(output, veorq_u8(low_product, high_product)),
                );
            }
        }
        for offset in (unrolled_len..vector_len).step_by(16) {
            let input = vld1q_u8(src.as_ptr().add(offset));
            let product = veorq_u8(
                vqtbl1q_u8(low_table, vandq_u8(input, nibble_mask)),
                vqtbl1q_u8(high_table, vshrq_n_u8(input, 4)),
            );
            let output = vld1q_u8(dst.as_ptr().add(offset));
            vst1q_u8(dst.as_mut_ptr().add(offset), veorq_u8(output, product));
        }

        vector_len
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod x86 {
    #[cfg(target_arch = "x86")]
    use core::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use core::arch::x86_64::*;

    #[target_feature(enable = "ssse3")]
    pub(super) unsafe fn addmul(
        dst: &mut [u8],
        src: &[u8],
        low: &[u8; 16],
        high: &[u8; 16],
    ) -> usize {
        let vector_len = dst.len() & !15;
        let unrolled_len = dst.len() & !63;
        let low_table = _mm_loadu_si128(low.as_ptr().cast());
        let high_table = _mm_loadu_si128(high.as_ptr().cast());
        let nibble_mask = _mm_set1_epi8(0x0f);

        for base in (0..unrolled_len).step_by(64) {
            for offset in [base, base + 16, base + 32, base + 48] {
                let input = _mm_loadu_si128(src.as_ptr().add(offset).cast());
                let low_product = _mm_shuffle_epi8(low_table, _mm_and_si128(input, nibble_mask));
                let high_indexes = _mm_and_si128(_mm_srli_epi16(input, 4), nibble_mask);
                let high_product = _mm_shuffle_epi8(high_table, high_indexes);
                let output = _mm_loadu_si128(dst.as_ptr().add(offset).cast());
                _mm_storeu_si128(
                    dst.as_mut_ptr().add(offset).cast(),
                    _mm_xor_si128(output, _mm_xor_si128(low_product, high_product)),
                );
            }
        }
        for offset in (unrolled_len..vector_len).step_by(16) {
            let input = _mm_loadu_si128(src.as_ptr().add(offset).cast());
            let low_product = _mm_shuffle_epi8(low_table, _mm_and_si128(input, nibble_mask));
            let high_indexes = _mm_and_si128(_mm_srli_epi16(input, 4), nibble_mask);
            let high_product = _mm_shuffle_epi8(high_table, high_indexes);
            let output = _mm_loadu_si128(dst.as_ptr().add(offset).cast());
            _mm_storeu_si128(
                dst.as_mut_ptr().add(offset).cast(),
                _mm_xor_si128(output, _mm_xor_si128(low_product, high_product)),
            );
        }

        vector_len
    }
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
mod wasm {
    use core::arch::wasm32::*;

    pub(super) unsafe fn addmul(
        dst: &mut [u8],
        src: &[u8],
        low: &[u8; 16],
        high: &[u8; 16],
    ) -> usize {
        let vector_len = dst.len() & !15;
        let unrolled_len = dst.len() & !63;
        let low_table = v128_load(low.as_ptr().cast());
        let high_table = v128_load(high.as_ptr().cast());
        let nibble_mask = u8x16_splat(0x0f);

        for base in (0..unrolled_len).step_by(64) {
            for offset in [base, base + 16, base + 32, base + 48] {
                let input = v128_load(src.as_ptr().add(offset).cast());
                let low_product = i8x16_swizzle(low_table, v128_and(input, nibble_mask));
                let high_product = i8x16_swizzle(high_table, u8x16_shr(input, 4));
                let output = v128_load(dst.as_ptr().add(offset).cast());
                v128_store(
                    dst.as_mut_ptr().add(offset).cast(),
                    v128_xor(output, v128_xor(low_product, high_product)),
                );
            }
        }
        for offset in (unrolled_len..vector_len).step_by(16) {
            let input = v128_load(src.as_ptr().add(offset).cast());
            let low_product = i8x16_swizzle(low_table, v128_and(input, nibble_mask));
            let high_product = i8x16_swizzle(high_table, u8x16_shr(input, 4));
            let output = v128_load(dst.as_ptr().add(offset).cast());
            v128_store(
                dst.as_mut_ptr().add(offset).cast(),
                v128_xor(output, v128_xor(low_product, high_product)),
            );
        }

        vector_len
    }
}
