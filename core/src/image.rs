// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use image::DynamicImage;
use image::RgbaImage;

pub trait IntoBgra8 {
    fn into_bgra8(self) -> RgbaImage;
}

impl IntoBgra8 for DynamicImage {
    fn into_bgra8(self) -> RgbaImage {
        let mut raw = self.into_rgba8();
        for i in 0..raw.len() / 4 {
            raw.swap(i * 4, i * 4 + 2);
        }

        raw
    }
}
