// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::ops::Range;
use std::sync::Arc;

use chrono::Local;
use cosmic_text::Align;
use cosmic_text::Attrs;
use cosmic_text::Buffer;
use cosmic_text::Color;
use cosmic_text::FontSystem;
use cosmic_text::Metrics;
use cosmic_text::Shaping;
use cosmic_text::SwashCache;
use cosmic_text::fontdb::Source;
use image::Rgba;
use image::RgbaImage;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio::task::block_in_place;

use crate::SCREEN_DIMENSIONS;
use crate::Update;
use crate::switchbot::SwitchBotData;
use crate::wallpaper::WallpaperData;

#[derive(Debug, Clone, Copy)]
enum TextAnchor {
    TopLeft,
    TopRight,
}

#[derive(Debug)]
struct DrawContext {
    back_buffer: RgbaImage,
    font_system: FontSystem,
    swash_cache: SwashCache,

    switchbot: Option<SwitchBotData>,
    wallpapar: Option<WallpaperData>,
}

impl DrawContext {
    fn new() -> anyhow::Result<Self> {
        let fonts = [Source::Binary(Arc::new(include_bytes!("../fonts/Lato-Bold.ttf")))];
        Ok(Self {
            back_buffer: RgbaImage::new(SCREEN_DIMENSIONS.0, SCREEN_DIMENSIONS.1),
            font_system: FontSystem::new_with_fonts(fonts),
            swash_cache: SwashCache::new(),
            switchbot: None,
            wallpapar: None,
        })
    }

    async fn draw(&mut self) -> anyhow::Result<()> {
        block_in_place(|| {
            const RECT_COLOR: Rgba<u8> = Rgba([0, 0, 0, 200]);
            const TEXT_COLOR: Color = Color::rgb(255, 255, 255);

            // Draw wallpaper
            if let Some(wallpaper) = &self.wallpapar {
                self.back_buffer.copy_from_slice(wallpaper.as_ref());
            } else {
                self.back_buffer.fill(0);
            }

            // Draw time and date
            let datetime = Local::now();

            self.fill_rect(50, 50, 880, 670, RECT_COLOR);
            self.fill_rect(1380, 50, 2510, 510, RECT_COLOR);

            let lines = (
                datetime.format("%H").to_string(),
                datetime.format("%M").to_string(),
                datetime.format("%S").to_string(),
            );
            self.draw_text(&lines.0, 480, 70, 260, TEXT_COLOR, TextAnchor::TopRight);
            self.draw_text(&lines.1, 480, 330, 260, TEXT_COLOR, TextAnchor::TopRight);
            self.draw_text(&lines.2, 560, 435, 150, TEXT_COLOR, TextAnchor::TopLeft);

            let lines = (
                datetime.format("%b %e, %Y").to_string().to_ascii_uppercase(),
                datetime.format("%a").to_string().to_ascii_uppercase(),
            );
            self.draw_text(&lines.0, 2400, 100, 140, TEXT_COLOR, TextAnchor::TopRight);
            self.draw_text(&lines.1, 2400, 280, 140, TEXT_COLOR, TextAnchor::TopRight);

            // Draw SwitchBot measurements
            self.fill_rect(50, 1025, 1670, 1550, RECT_COLOR);

            if let Some(SwitchBotData { indoor, outdoor, tank }) = self.switchbot {
                let lines = (format!("{:.1}", indoor.temperature), format!("{:}", indoor.humidity));
                self.draw_text("IN", 200, 1080, 80, TEXT_COLOR, TextAnchor::TopLeft);
                self.draw_text(&lines.0, 430, 1200, 120, TEXT_COLOR, TextAnchor::TopRight);
                self.draw_text("°C", 540, 1235, 80, TEXT_COLOR, TextAnchor::TopRight);
                self.draw_text(&lines.1, 430, 1355, 120, TEXT_COLOR, TextAnchor::TopRight);
                self.draw_text("%", 540, 1390, 80, TEXT_COLOR, TextAnchor::TopRight);

                let lines = (format!("{:.1}", outdoor.temperature), format!("{:}", outdoor.humidity));
                self.draw_text("OUT", 700, 1080, 80, TEXT_COLOR, TextAnchor::TopLeft);
                self.draw_text(&lines.0, 930, 1200, 120, TEXT_COLOR, TextAnchor::TopRight);
                self.draw_text("°C", 1040, 1235, 80, TEXT_COLOR, TextAnchor::TopRight);
                self.draw_text(&lines.1, 930, 1355, 120, TEXT_COLOR, TextAnchor::TopRight);
                self.draw_text("%", 1040, 1390, 80, TEXT_COLOR, TextAnchor::TopRight);

                let lines = (format!("{:.1}", tank.temperature), format!("{:}", tank.humidity));
                self.draw_text("CAGE", 1200, 1080, 80, TEXT_COLOR, TextAnchor::TopLeft);
                self.draw_text(&lines.0, 1430, 1200, 120, TEXT_COLOR, TextAnchor::TopRight);
                self.draw_text("°C", 1540, 1235, 80, TEXT_COLOR, TextAnchor::TopRight);
                self.draw_text(&lines.1, 1430, 1355, 120, TEXT_COLOR, TextAnchor::TopRight);
                self.draw_text("%", 1540, 1390, 80, TEXT_COLOR, TextAnchor::TopRight);
            }
        });

        // Flip
        self.flip().await?;

        Ok(())
    }

    fn fill_rect(&mut self, l: u32, t: u32, r: u32, b: u32, color: Rgba<u8>) {
        let (l, r) = (u32::min(l, r), u32::max(l, r));
        let (t, b) = (u32::min(t, b), u32::max(t, b));

        for y in t..b {
            for x in l..r {
                let pixel = self.back_buffer.get_pixel_mut(x, y);
                *pixel = alphablend(*pixel, color);
            }
        }
    }

    fn draw_text(&mut self, text: &str, x: u32, y: u32, size: u32, color: Color, anchor: TextAnchor) {
        let align = match anchor {
            TextAnchor::TopLeft => Align::Left,
            TextAnchor::TopRight => Align::Right,
        };

        let metrics = Metrics::new(size as f32, size as f32 * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        let mut buffer = buffer.borrow_with(&mut self.font_system);

        buffer.set_size(Some(SCREEN_DIMENSIONS.0 as f32), None);
        buffer.set_text(text, &Attrs::new(), Shaping::Advanced, Some(align));

        buffer.draw(&mut self.swash_cache, color, |bx, by, w, h, color| {
            const RANGE_X: Range<i32> = 0..SCREEN_DIMENSIONS.0 as i32;
            const RANGE_Y: Range<i32> = 0..SCREEN_DIMENSIONS.0 as i32;

            let (x, y) = match anchor {
                TextAnchor::TopLeft => (x as i32 + bx, y as i32 + by),
                TextAnchor::TopRight => (x as i32 - SCREEN_DIMENSIONS.0 as i32 + bx, y as i32 + by),
            };
            if !RANGE_X.contains(&x) || !RANGE_Y.contains(&y) || w != 1 || h != 1 {
                return;
            }

            let pixel = self.back_buffer.get_pixel_mut(x as u32, y as u32);
            *pixel = alphablend(*pixel, Rgba(color.as_rgba()));
        });
    }

    async fn flip(&mut self) -> anyhow::Result<()> {
        let mut dev = OpenOptions::new().write(true).open("/dev/fb0").await?;
        dev.write_all(&self.back_buffer).await?;

        Ok(())
    }
}

#[inline]
fn alphablend_channel(src: u8, dst: u8, alpha: u8) -> u8 {
    let src = src as u32;
    let dst = dst as u32;
    let alpha = alpha as u32;
    ((src * (255 - alpha) / 255) + (dst * alpha / 255)).clamp(0, 255) as u8
}

#[inline]
fn alphablend(src: Rgba<u8>, dst: Rgba<u8>) -> Rgba<u8> {
    Rgba([
        alphablend_channel(src[0], dst[0], dst[3]),
        alphablend_channel(src[1], dst[1], dst[3]),
        alphablend_channel(src[2], dst[2], dst[3]),
        255,
    ])
}

pub async fn worker(mut receiver: mpsc::Receiver<Update>) -> anyhow::Result<()> {
    let mut context = DrawContext::new()?;

    while let Some(update) = receiver.recv().await {
        match update {
            Update::Tick => {}
            Update::SwitchBot(data) => context.switchbot = Some(data),
            Update::Wallpaper(data) => context.wallpapar = Some(data),
        }

        context.draw().await?;
    }

    Ok(())
}
