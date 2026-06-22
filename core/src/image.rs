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
        Self {
            data: avec![[CACHELINE_ALIGN]| 0; (width * height * 4) as usize],
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
        let i = ((y * self.width + x) * 4) as usize;
        <Rgba<u8> as Pixel>::from_slice_mut(&mut self.data[i..(i + 4)])
    }

    #[inline]
    pub fn get_pixel_mut_checked(&mut self, x: u32, y: u32) -> Option<&mut Rgba<u8>> {
        if x >= self.width || y >= self.height {
            return None;
        }

        let i = ((y * self.width + x) * 4) as usize;
        Some(<Rgba<u8> as Pixel>::from_slice_mut(&mut self.data[i..(i + 4)]))
    }
}

impl From<DynamicImage> for AlignedImage {
    #[allow(clippy::cast_ptr_alignment)]
    fn from(value: DynamicImage) -> Self {
        // Dynamic image to RGBA Bitmap
        let image = value.into_rgba8();
        let mut data = AVec::from_slice(CACHELINE_ALIGN, &image);
        debug_assert!(data.len().is_multiple_of(64));

        // Convert from RGBA to BGRA
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

        Self {
            data,
            width: image.width(),
            height: image.height(),
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
