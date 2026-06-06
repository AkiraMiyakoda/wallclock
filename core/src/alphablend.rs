// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::arch::x86_64::*;

#[inline]
pub fn alphablend_x4(src: &[u8], dst: &mut [u8]) {
    debug_assert!(src.len() >= 4 * 4);
    debug_assert!(dst.len() >= 4 * 4);

    unsafe {
        // Load 4 pixels each from src and dst
        let src_8x4 = _mm_load_si128(src.as_ptr() as *const __m128i);
        let dst_8x4 = _mm_load_si128(dst.as_ptr() as *const __m128i);

        // Unpack each channel to 16bit (lo, hi)
        let zero = _mm_setzero_si128();
        let src_16x4 = (_mm_unpacklo_epi8(src_8x4, zero), _mm_unpackhi_epi8(src_8x4, zero));
        let dst_16x4 = (_mm_unpacklo_epi8(dst_8x4, zero), _mm_unpackhi_epi8(dst_8x4, zero));

        // Copy the alpha value to all channels
        let alpha_16x4 = (
            _mm_and_si128(src_16x4.0, _mm_set_epi16(-1, 0, 0, 0, -1, 0, 0, 0)),
            _mm_and_si128(src_16x4.1, _mm_set_epi16(-1, 0, 0, 0, -1, 0, 0, 0)),
        );
        let alpha_16x4 = (
            _mm_or_si128(alpha_16x4.0, _mm_srli_si128(alpha_16x4.0, 2)),
            _mm_or_si128(alpha_16x4.1, _mm_srli_si128(alpha_16x4.1, 2)),
        );
        let alpha_16x4 = (
            _mm_or_si128(alpha_16x4.0, _mm_srli_si128(alpha_16x4.0, 4)),
            _mm_or_si128(alpha_16x4.1, _mm_srli_si128(alpha_16x4.1, 4)),
        );

        // dst = src * a + dst * (255 - a)
        let src_16x4 = (
            _mm_mullo_epi16(src_16x4.0, alpha_16x4.0),
            _mm_mullo_epi16(src_16x4.1, alpha_16x4.1),
        );
        let dst_16x4 = (
            _mm_mullo_epi16(dst_16x4.0, _mm_sub_epi16(_mm_set1_epi16(255), alpha_16x4.0)),
            _mm_mullo_epi16(dst_16x4.1, _mm_sub_epi16(_mm_set1_epi16(255), alpha_16x4.1)),
        );
        let dst_16x4 = (
            _mm_add_epi16(src_16x4.0, dst_16x4.0),
            _mm_add_epi16(src_16x4.1, dst_16x4.1),
        );

        // dst = (dst + 1 + (dst >> 8)) >> 8
        let dst_16x4 = (
            _mm_add_epi16(dst_16x4.0, _mm_set1_epi16(1)),
            _mm_add_epi16(dst_16x4.1, _mm_set1_epi16(1)),
        );
        let dst_16x4 = (
            _mm_add_epi16(dst_16x4.0, _mm_srli_epi16(dst_16x4.0, 8)),
            _mm_add_epi16(dst_16x4.1, _mm_srli_epi16(dst_16x4.0, 8)),
        );
        let dst_16x4 = (_mm_srli_epi16(dst_16x4.0, 8), _mm_srli_epi16(dst_16x4.1, 8));

        // Store the result
        _mm_store_si128(
            dst.as_mut_ptr() as *mut __m128i,
            _mm_packus_epi16(dst_16x4.0, dst_16x4.1),
        );
    }
}
