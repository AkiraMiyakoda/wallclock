// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::fs::File;
use std::fs::OpenOptions;
use std::os::fd::AsFd;
use std::os::fd::BorrowedFd;
use std::sync::Arc;

use anyhow::anyhow;
use chrono::Local;
use compact_str::ToCompactString;
use compact_str::format_compact;
use cosmic_text::Align;
use cosmic_text::Attrs;
use cosmic_text::Buffer;
use cosmic_text::Color;
use cosmic_text::Family;
use cosmic_text::FontSystem;
use cosmic_text::Metrics;
use cosmic_text::Shaping;
use cosmic_text::SwashCache;
use cosmic_text::Weight;
use cosmic_text::fontdb::Source;
use drm::Device as DrmDevice;
use drm::buffer::DrmFourcc;
use drm::control::Device as ControlDevice;
use drm::control::connector;
use drm::control::dumbbuffer::DumbBuffer;
use drm::control::framebuffer;
use format::WithCommas;
use image::Pixel;
use image::Rgba;
use image::RgbaImage;
use log::error;
use tokio::sync::mpsc;
use tokio::task::block_in_place;

use crate::SCREEN_DIMENSIONS;
use crate::Update;
use crate::openweather::OpenWeatherData;
use crate::settings;
use crate::switchbot::SwitchBotData;
use crate::wallpaper::WallpaperData;

#[derive(Debug)]
struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl DrmDevice for Card {}
impl ControlDevice for Card {}

#[derive(Debug, Clone, Copy)]
enum TextAnchor {
    TopLeft,
    #[allow(dead_code)]
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

#[derive(Debug)]
struct DrawContext {
    card: Card,
    dumb_buffer: DumbBuffer,
    frame_buffer: framebuffer::Handle,
    back_buffer: RgbaImage,
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl Drop for DrawContext {
    fn drop(&mut self) {
        let _ = self.card.destroy_framebuffer(self.frame_buffer);
        let _ = self.card.destroy_dumb_buffer(self.dumb_buffer);
    }
}

impl DrawContext {
    fn new() -> anyhow::Result<Self> {
        // Open DRM device
        let settings::Drm { device } = settings::drm();
        let card = Card(OpenOptions::new().read(true).write(true).open(device)?);

        // Get DRM related resouces
        let resources = card.resource_handles()?;
        let connector = resources
            .connectors()
            .iter()
            .flat_map(|conn| card.get_connector(*conn, true))
            .find(|conn| conn.state() == connector::State::Connected)
            .ok_or(anyhow!("Connector not found"))?;
        let mode = *connector
            .modes()
            .iter()
            .find(|mode| mode.size() == SCREEN_DIMENSIONS.into())
            .ok_or(anyhow!("Appropriate mode not found"))?;
        let crtc = resources
            .crtcs()
            .iter()
            .flat_map(|crtc| card.get_crtc(*crtc))
            .next()
            .ok_or(anyhow!("CRTC not found"))?;

        // Create frame buffer
        let dumb_buffer = card.create_dumb_buffer(SCREEN_DIMENSIONS.into(), DrmFourcc::Xrgb8888, 32)?;
        let frame_buffer = card.add_framebuffer(&dumb_buffer, 32, 32).unwrap();

        card.set_crtc(
            crtc.handle(),
            Some(frame_buffer),
            (0, 0),
            &[connector.handle()],
            Some(mode),
        )?;

        let fonts = [Source::Binary(Arc::new(include_bytes!("../fonts/Lato-Bold.ttf")))];
        let (screen_width, screen_height) = SCREEN_DIMENSIONS.into();
        Ok(Self {
            card,
            dumb_buffer,
            frame_buffer,
            back_buffer: RgbaImage::new(screen_width, screen_height),
            font_system: FontSystem::new_with_fonts(fonts),
            swash_cache: SwashCache::new(),
        })
    }

    fn draw(&mut self, bundle: &DataBundle) -> anyhow::Result<()> {
        const RECT_COLOR: Rgba<u8> = Rgba([0, 0, 0, 200]);
        const TEXT_COLOR: Color = Color::rgb(255, 255, 255);

        // Draw wallpaper
        if let Some(wallpaper) = &bundle.wallpapar {
            self.back_buffer.copy_from_slice(wallpaper.image.as_raw());
        } else {
            self.back_buffer.fill(0);
        }

        // Draw time and date
        self.fill_rect(50, 50, 880, 750, RECT_COLOR);
        self.fill_rect(1380, 50, 2510, 510, RECT_COLOR);

        let datetime = Local::now();
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

        if let Some(data) = &bundle.switchbot {
            let lines = (
                format_compact!("{:.1}", data.indoor.temperature),
                format_compact!("{:}", data.indoor.humidity),
            );
            self.draw_text("IN", 160, 1110, 80, TEXT_COLOR, TextAnchor::TopLeft);
            self.draw_text(&lines.0, 370, 1360, 110, TEXT_COLOR, TextAnchor::BottomRight);
            self.draw_text("°C", 410, 1355, 80, TEXT_COLOR, TextAnchor::BottomLeft);
            self.draw_text(&lines.1, 370, 1500, 110, TEXT_COLOR, TextAnchor::BottomRight);
            self.draw_text("%", 430, 1495, 80, TEXT_COLOR, TextAnchor::BottomLeft);

            let lines = (
                format_compact!("{:.1}", data.outdoor.temperature),
                format_compact!("{:}", data.outdoor.humidity),
            );
            self.draw_text("OUT", 640, 1110, 80, TEXT_COLOR, TextAnchor::TopLeft);
            self.draw_text(&lines.0, 850, 1360, 110, TEXT_COLOR, TextAnchor::BottomRight);
            self.draw_text("°C", 890, 1355, 80, TEXT_COLOR, TextAnchor::BottomLeft);
            self.draw_text(&lines.1, 850, 1500, 110, TEXT_COLOR, TextAnchor::BottomRight);
            self.draw_text("%", 910, 1495, 80, TEXT_COLOR, TextAnchor::BottomLeft);

            let lines = (
                format_compact!("{:.1}", data.tank.temperature),
                format_compact!("{:}", data.tank.humidity),
            );
            self.draw_text("CAGE", 1110, 1110, 80, TEXT_COLOR, TextAnchor::TopLeft);
            self.draw_text(&lines.0, 1320, 1360, 110, TEXT_COLOR, TextAnchor::BottomRight);
            self.draw_text("°C", 1370, 1355, 80, TEXT_COLOR, TextAnchor::BottomLeft);
            self.draw_text(&lines.1, 1330, 1500, 110, TEXT_COLOR, TextAnchor::BottomRight);
            self.draw_text("%", 1390, 1495, 80, TEXT_COLOR, TextAnchor::BottomLeft);
        }

        // Draw OpenWeather measurements
        self.fill_rect(1600, 860, 2510, 1550, RECT_COLOR);

        if let Some(data) = &bundle.openweather {
            self.draw_image(1905, 930, &data.icon);

            let lines = (
                data.description.to_ascii_uppercase(),
                format_compact!("{}", WithCommas::from(data.pressure)),
            );

            self.draw_text(&lines.0, 2055, 1340, 80, TEXT_COLOR, TextAnchor::BottomCenter);

            self.draw_text(&lines.1, 2100, 1500, 105, TEXT_COLOR, TextAnchor::BottomRight);
            self.draw_text("hPA", 2130, 1495, 75, TEXT_COLOR, TextAnchor::BottomLeft);
        }

        // Flip
        {
            let mut map = self.card.map_dumb_buffer(&mut self.dumb_buffer)?;
            map.copy_from_slice(&self.back_buffer);
        }

        Ok(())
    }

    fn fill_rect(&mut self, l: u32, t: u32, r: u32, b: u32, color: Rgba<u8>) {
        if l >= r || t >= b {
            return;
        }

        for y in t..b {
            for x in l..r {
                if let Some(pixel) = self.back_buffer.get_pixel_mut_checked(x, y) {
                    pixel.blend(&color);
                }
            }
        }
    }

    fn draw_image(&mut self, x: u32, y: u32, image: &RgbaImage) {
        for bx in 0..image.width() {
            for by in 0..image.height() {
                let x = x + bx;
                let y = y + by;

                if let Some(pixel) = self.back_buffer.get_pixel_mut_checked(x, y) {
                    pixel.blend(image.get_pixel(bx, by));
                }
            }
        }
    }

    fn draw_text(&mut self, text: &str, x: u32, y: u32, size: u32, color: Color, anchor: TextAnchor) {
        let metrics = Metrics::new(size as f32, size as f32 * 1.2);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        let mut buffer = buffer.borrow_with(&mut self.font_system);

        let attrs = Attrs::new().family(Family::Name("Lato")).weight(Weight::BOLD);
        buffer.set_text(text, &attrs, Shaping::Advanced, Some(Align::Left));

        let width = buffer
            .layout_runs()
            .map(|run| run.line_w.ceil() as i32)
            .max()
            .unwrap_or(0);
        let height: i32 = buffer.layout_runs().map(|run| run.line_height.ceil() as i32).sum();

        buffer.draw(&mut self.swash_cache, color, |bx, by, w, h, color| {
            if w != 1 || h != 1 {
                return;
            }

            let x = match anchor {
                TextAnchor::TopLeft | TextAnchor::BottomLeft => x as i32 + bx,
                TextAnchor::TopCenter | TextAnchor::BottomCenter => x as i32 - width / 2 + bx,
                TextAnchor::TopRight | TextAnchor::BottomRight => x as i32 - width + bx,
            };
            let y = match anchor {
                TextAnchor::TopLeft | TextAnchor::TopCenter | TextAnchor::TopRight => y as i32 + by,
                TextAnchor::BottomLeft | TextAnchor::BottomCenter | TextAnchor::BottomRight => y as i32 - height + by,
            };

            if let Some(pixel) = self.back_buffer.get_pixel_mut_checked(x as u32, y as u32) {
                pixel.blend(&Rgba(color.as_rgba()));
            }
        });
    }
}

#[derive(Debug)]
struct DataBundle {
    switchbot: Option<SwitchBotData>,
    openweather: Option<OpenWeatherData>,
    wallpapar: Option<WallpaperData>,
}

impl DataBundle {
    fn new() -> Self {
        Self {
            switchbot: None,
            openweather: None,
            wallpapar: None,
        }
    }
}

pub async fn worker(mut receiver: mpsc::Receiver<Update>) -> anyhow::Result<()> {
    let mut context = DrawContext::new()?;
    let mut bundle = DataBundle::new();

    while let Some(update) = receiver.recv().await {
        match update {
            Update::Tick => {}
            Update::SwitchBot(data) => bundle.switchbot = Some(data),
            Update::OpenWeather(data) => bundle.openweather = Some(data),
            Update::Wallpaper(data) => bundle.wallpapar = Some(data),
        }

        if let Err(e) = block_in_place(|| context.draw(&bundle)) {
            error!("Failed to draw: {e:?}")
        }
    }

    Ok(())
}
