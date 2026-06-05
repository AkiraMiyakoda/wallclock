// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::fs::OpenOptions;
use std::io::Write;
use std::ops::Range;
use std::sync::Arc;

use chrono::Local;
use compact_str::ToCompactString;
use compact_str::format_compact;
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
use log::error;
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
    BottomLeft,
    BottomRight,
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
        let fonts = [Source::Binary(Arc::new(include_bytes!("../fonts/Lato-Regular.ttf")))];
        Ok(Self {
            back_buffer: RgbaImage::new(SCREEN_DIMENSIONS.0, SCREEN_DIMENSIONS.1),
            font_system: FontSystem::new_with_fonts(fonts),
            swash_cache: SwashCache::new(),
            switchbot: None,
            wallpapar: None,
        })
    }

    fn draw(&mut self) -> anyhow::Result<()> {
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

        self.fill_rect(50, 50, 880, 750, RECT_COLOR);
        self.fill_rect(1380, 50, 2510, 510, RECT_COLOR);

        let lines = (
            datetime.format("%H").to_compact_string(),
            datetime.format("%M").to_compact_string(),
            datetime.format("%S").to_compact_string(),
        );
        self.draw_text(&lines.0, 510, 420, 300, TEXT_COLOR, TextAnchor::BottomRight);
        self.draw_text(&lines.1, 510, 730, 300, TEXT_COLOR, TextAnchor::BottomRight);
        self.draw_text(&lines.2, 570, 700, 150, TEXT_COLOR, TextAnchor::BottomLeft);

        let lines = (
            datetime.format("%b %e, %Y").to_compact_string().to_ascii_uppercase(),
            datetime.format("%a").to_compact_string().to_ascii_uppercase(),
        );
        self.draw_text(&lines.0, 2400, 100, 140, TEXT_COLOR, TextAnchor::TopRight);
        self.draw_text(&lines.1, 2400, 280, 140, TEXT_COLOR, TextAnchor::TopRight);

        // Draw SwitchBot measurements
        self.fill_rect(50, 1060, 1550, 1550, RECT_COLOR);

        if let Some(SwitchBotData { indoor, outdoor, tank }) = self.switchbot.clone() {
            let lines = (
                format_compact!("{:.1}", indoor.temperature),
                format_compact!("{:}", indoor.humidity),
            );
            self.draw_text("IN", 160, 1110, 80, TEXT_COLOR, TextAnchor::TopLeft);
            self.draw_text(&lines.0, 370, 1370, 110, TEXT_COLOR, TextAnchor::BottomRight);
            self.draw_text("°C", 410, 1365, 80, TEXT_COLOR, TextAnchor::BottomLeft);
            self.draw_text(&lines.1, 370, 1510, 110, TEXT_COLOR, TextAnchor::BottomRight);
            self.draw_text("%", 430, 1505, 80, TEXT_COLOR, TextAnchor::BottomLeft);

            let lines = (
                format_compact!("{:.1}", outdoor.temperature),
                format_compact!("{:}", outdoor.humidity),
            );
            self.draw_text("OUT", 640, 1110, 80, TEXT_COLOR, TextAnchor::TopLeft);
            self.draw_text(&lines.0, 850, 1370, 110, TEXT_COLOR, TextAnchor::BottomRight);
            self.draw_text("°C", 890, 1365, 80, TEXT_COLOR, TextAnchor::BottomLeft);
            self.draw_text(&lines.1, 850, 1510, 110, TEXT_COLOR, TextAnchor::BottomRight);
            self.draw_text("%", 910, 1505, 80, TEXT_COLOR, TextAnchor::BottomLeft);

            let lines = (
                format_compact!("{:.1}", tank.temperature),
                format_compact!("{:}", tank.humidity),
            );
            self.draw_text("CAGE", 1110, 1110, 80, TEXT_COLOR, TextAnchor::TopLeft);
            self.draw_text(&lines.0, 1320, 1370, 110, TEXT_COLOR, TextAnchor::BottomRight);
            self.draw_text("°C", 1370, 1365, 80, TEXT_COLOR, TextAnchor::BottomLeft);
            self.draw_text(&lines.1, 1330, 1510, 110, TEXT_COLOR, TextAnchor::BottomRight);
            self.draw_text("%", 1390, 1505, 80, TEXT_COLOR, TextAnchor::BottomLeft);
        }

        // Flip
        {
            let mut fb = OpenOptions::new().write(true).open("/dev/fb0")?;
            fb.write_all(&self.back_buffer)?;
        }

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
        let metrics = Metrics::new(size as f32, size as f32 * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        let mut buffer = buffer.borrow_with(&mut self.font_system);

        buffer.set_text(text, &Attrs::new(), Shaping::Advanced, Some(Align::Left));

        let width = buffer
            .layout_runs()
            .map(|run| run.line_w.ceil() as i32)
            .max()
            .unwrap_or(0);
        let height: i32 = buffer.layout_runs().map(|run| run.line_height.ceil() as i32).sum();

        buffer.draw(&mut self.swash_cache, color, |bx, by, w, h, color| {
            const RANGE_X: Range<i32> = 0..SCREEN_DIMENSIONS.0 as i32;
            const RANGE_Y: Range<i32> = 0..SCREEN_DIMENSIONS.0 as i32;

            let x = match anchor {
                TextAnchor::TopLeft | TextAnchor::BottomLeft => x as i32 + bx,
                TextAnchor::TopRight | TextAnchor::BottomRight => x as i32 - width + bx,
            };
            let y = match anchor {
                TextAnchor::TopLeft | TextAnchor::TopRight => y as i32 + by,
                TextAnchor::BottomLeft | TextAnchor::BottomRight => y as i32 - height + by,
            };

            if !RANGE_X.contains(&x) || !RANGE_Y.contains(&y) || w != 1 || h != 1 {
                return;
            }

            let pixel = self.back_buffer.get_pixel_mut(x as u32, y as u32);
            *pixel = alphablend(*pixel, Rgba(color.as_rgba()));
        });
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

        if let Err(e) = block_in_place(|| context.draw()) {
            error!("Failed to draw: {e:?}")
        }
    }

    Ok(())
}
