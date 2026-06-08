// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::arch::x86_64::*;
use std::ops::Deref;
use std::ops::DerefMut;

use aligned_vec::AVec;
use aligned_vec::CACHELINE_ALIGN;
use aligned_vec::avec;
use image::DynamicImage;
use image::Pixel;
use image::Rgba;

#[derive(Debug)]
pub struct AlignedRgbaImage {
    data: AVec<u8>,
    width: u32,
    height: u32,
}

impl AlignedRgbaImage {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            data: avec![[CACHELINE_ALIGN]| 0; (width * height * 4) as usize],
            width,
            height,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn get_pixel_mut(&mut self, x: u32, y: u32) -> &mut Rgba<u8> {
        let i = ((y * self.width + x) * 4) as usize;
        <Rgba<u8> as Pixel>::from_slice_mut(&mut self.data[i..(i + 4)])
    }

    pub fn get_pixel_mut_checked(&mut self, x: u32, y: u32) -> Option<&mut Rgba<u8>> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let i = ((y * self.width + x) * 4) as usize;
        Some(<Rgba<u8> as Pixel>::from_slice_mut(&mut self.data[i..(i + 4)]))
    }
}

impl From<DynamicImage> for AlignedRgbaImage {
    fn from(value: DynamicImage) -> Self {
        // Dynamic image to RGBA Bitmap
        let image = value.into_rgba8();
        let mut data = AVec::from_slice(CACHELINE_ALIGN, &image);

        // Convert from RGBA to BGRA
        unsafe {
            #[rustfmt::skip]
            let shuffle_mask = _mm256_set_epi8(
                15, 12, 13, 14, 11, 8, 9, 10, 7, 4, 5, 6, 3, 0, 1, 2,
                15, 12, 13, 14, 11, 8, 9, 10, 7, 4, 5, 6, 3, 0, 1, 2,
            );
            let mut ptr = data.as_mut_ptr();

            for _ in 0..data.len() / 32 {
                let row = _mm256_load_si256(ptr as *const __m256i);
                let row = _mm256_shuffle_epi8(row, shuffle_mask);
                _mm256_store_si256(ptr as *mut __m256i, row);

                ptr = ptr.add(32);
            }
        }

        Self {
            data,
            width: image.width(),
            height: image.height(),
        }
    }
}

impl Deref for AlignedRgbaImage {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for AlignedRgbaImage {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data.deref_mut()
    }
}

impl AsMut<[u8]> for AlignedRgbaImage {
    fn as_mut(&mut self) -> &mut [u8] {
        self.data.as_mut()
    }
}

impl AsRef<[u8]> for AlignedRgbaImage {
    fn as_ref(&self) -> &[u8] {
        self.data.as_ref()
    }
}

#[inline]
pub fn alphablend_x8<T, U>(src: &[T], dst: &mut [U]) {
    debug_assert!(size_of_val(src) >= 32);
    debug_assert!(size_of_val(dst) >= 32);

    unsafe {
        #[rustfmt::skip]
        let shuffle_mask = _mm256_set_epi8(
            15, 15, 15, 15, 11, 11, 11, 11, 7, 7, 7, 7, 3, 3, 3, 3,
            15, 15, 15, 15, 11, 11, 11, 11, 7, 7, 7, 7, 3, 3, 3, 3,
        );
        let alpha_mask = _mm256_set1_epi32(0xff000000_u32 as i32);

        // Load 8 pixels each from src and dst
        let src_8x8 = _mm256_load_si256(src.as_ptr() as *const __m256i);

        // Do nothing if all alpha values are zero
        let alpha = _mm256_and_si256(src_8x8, alpha_mask);
        if _mm256_testz_si256(alpha, alpha) != 0 {
            return;
        }

        let dst_8x8 = _mm256_load_si256(dst.as_ptr() as *const __m256i);

        // Unpack each channel to 16bit (lo, hi)
        let zero = _mm256_setzero_si256();
        let src_16x8 = (_mm256_unpacklo_epi8(src_8x8, zero), _mm256_unpackhi_epi8(src_8x8, zero));
        let dst_16x8 = (_mm256_unpacklo_epi8(dst_8x8, zero), _mm256_unpackhi_epi8(dst_8x8, zero));

        // Unpack alpha value
        let alpha_8x8 = _mm256_shuffle_epi8(src_8x8, shuffle_mask);
        let alpha_16x8 = (
            _mm256_unpacklo_epi8(alpha_8x8, zero),
            _mm256_unpackhi_epi8(alpha_8x8, zero),
        );

        // dst = (src * a + dst * (255 - a))
        let src_16x8 = (
            _mm256_mullo_epi16(src_16x8.0, alpha_16x8.0),
            _mm256_mullo_epi16(src_16x8.1, alpha_16x8.1),
        );
        let dst_16x8 = (
            _mm256_mullo_epi16(dst_16x8.0, _mm256_sub_epi16(_mm256_set1_epi16(255), alpha_16x8.0)),
            _mm256_mullo_epi16(dst_16x8.1, _mm256_sub_epi16(_mm256_set1_epi16(255), alpha_16x8.1)),
        );
        let dst_16x8 = (
            _mm256_add_epi16(src_16x8.0, dst_16x8.0),
            _mm256_add_epi16(src_16x8.1, dst_16x8.1),
        );

        // dst = (dst + (dst >> 8) + 1) >> 8
        let dst_16x8 = (
            _mm256_add_epi16(dst_16x8.0, _mm256_srli_epi16(dst_16x8.0, 8)),
            _mm256_add_epi16(dst_16x8.1, _mm256_srli_epi16(dst_16x8.1, 8)),
        );
        let dst_16x8 = (
            _mm256_add_epi16(dst_16x8.0, _mm256_set1_epi16(1)),
            _mm256_add_epi16(dst_16x8.1, _mm256_set1_epi16(1)),
        );
        let dst_16x8 = (_mm256_srli_epi16(dst_16x8.0, 8), _mm256_srli_epi16(dst_16x8.1, 8));

        // Store the result
        _mm256_store_si256(
            dst.as_mut_ptr() as *mut __m256i,
            _mm256_packus_epi16(dst_16x8.0, dst_16x8.1),
        );
    }
}
