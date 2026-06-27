// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

#[allow(clippy::wildcard_imports)]
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
pub struct AlignedImage {
    data: AVec<u8>,
    width: u32,
    height: u32,
}

impl AlignedImage {
    pub fn new(width: u32, height: u32) -> Self {
        assert!(width.is_multiple_of(16));

        Self {
            data: avec![[CACHELINE_ALIGN]| 0; width as usize * height as usize * 4],
            width,
            height,
        }
    }

    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[inline]
    pub fn stride(&self) -> u32 {
        self.width * 4
    }

    #[inline]
    pub fn get_pixel_mut(&mut self, x: u32, y: u32) -> &mut Rgba<u8> {
        debug_assert!(x < self.width);
        debug_assert!(y < self.height);

        let i = self.get_pixel_offset(x, y);
        <Rgba<u8> as Pixel>::from_slice_mut(&mut self.data[i..(i + 4)])
    }

    #[inline]
    pub fn get_pixel_mut_checked(&mut self, x: u32, y: u32) -> Option<&mut Rgba<u8>> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let i = self.get_pixel_offset(x, y);
        Some(<Rgba<u8> as Pixel>::from_slice_mut(&mut self.data[i..(i + 4)]))
    }

    #[inline]
    fn get_pixel_offset(&self, x: u32, y: u32) -> usize {
        ((y as usize * self.width as usize) + x as usize) * 4
    }
}

impl From<DynamicImage> for AlignedImage {
    fn from(value: DynamicImage) -> Self {
        assert!(value.width().is_multiple_of(16));

        // Convert a dynamic image to an RGBA bitmap
        let image = value.into_rgba8();
        let mut data = AVec::from_slice(CACHELINE_ALIGN, &image);
        assert!(data.len().is_multiple_of(64));

        // Convert from RGBA to BGRA as required by the renderer
        // SAFETY: This program is only supported on Zen 4 systems.
        // `data` is allocated with cache-line alignment, and the width invariant
        // ensures the buffer length is a multiple of 64 bytes.
        unsafe {
            rgba_to_bgra_avx512(&mut data);
        }

        Self {
            data,
            width: image.width(),
            height: image.height(),
        }
    }
}

#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn rgba_to_bgra_avx512(data: &mut AVec<u8>) {
    debug_assert!((data.as_ptr() as usize).is_multiple_of(64));
    debug_assert!(data.len().is_multiple_of(64));

    unsafe {
        #[rustfmt::skip]
        let shuffle_mask = _mm512_set_epi8(
            15, 12, 13, 14, 11, 8, 9, 10, 7, 4, 5, 6, 3, 0, 1, 2,
            15, 12, 13, 14, 11, 8, 9, 10, 7, 4, 5, 6, 3, 0, 1, 2,
            15, 12, 13, 14, 11, 8, 9, 10, 7, 4, 5, 6, 3, 0, 1, 2,
            15, 12, 13, 14, 11, 8, 9, 10, 7, 4, 5, 6, 3, 0, 1, 2,
        );

        let mut ptr: *mut __m512i = data.as_mut_ptr().cast();

        for _ in 0..data.len() / 64 {
            let row = _mm512_load_si512(ptr);
            let row = _mm512_shuffle_epi8(row, shuffle_mask);
            _mm512_store_si512(ptr, row);

            ptr = ptr.add(1);
        }
    }
}

impl Deref for AlignedImage {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for AlignedImage {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data.deref_mut()
    }
}

impl AsMut<[u8]> for AlignedImage {
    fn as_mut(&mut self) -> &mut [u8] {
        self.data.as_mut()
    }
}

impl AsRef<[u8]> for AlignedImage {
    fn as_ref(&self) -> &[u8] {
        self.data.as_ref()
    }
}
