// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

#[allow(clippy::wildcard_imports)]
use std::arch::x86_64::*;
use std::collections::VecDeque;
use std::fs::File;
use std::fs::OpenOptions;
use std::os::fd::AsFd;
use std::os::fd::BorrowedFd;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::anyhow;
use az::SaturatingAs;
use chrono::Local;
use chrono::Utc;
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
use log::error;
use num_traits::ToPrimitive;
use tokio::select;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio::task::block_in_place;
use tokio::time::MissedTickBehavior;
use tokio::time::interval;

use crate::SCREEN_DIMENSIONS;
use crate::Update;
use crate::image::AlignedImage;
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
    back_buffer: AlignedImage,
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
            back_buffer: AlignedImage::new(screen_width, screen_height),
            font_system: FontSystem::new_with_fonts(fonts),
            swash_cache: SwashCache::new(),
        })
    }
}

static SWITCHBOT: LazyLock<RwLock<Option<SwitchBotData>>> = LazyLock::new(|| RwLock::new(None));
static OPENWEATHER: LazyLock<RwLock<Option<OpenWeatherData>>> = LazyLock::new(|| RwLock::new(None));
static WALLPAPER: LazyLock<RwLock<VecDeque<WallpaperData>>> = LazyLock::new(|| RwLock::new(VecDeque::new()));

pub async fn worker(receiver: mpsc::Receiver<Update>) -> anyhow::Result<()> {
    select! {
        result = draw_worker() => result,
        result = update_worker(receiver) => result,
    }
}

async fn draw_worker() -> anyhow::Result<()> {
    const NANOS_PER_SEC: u64 = 1_000_000_000;

    let mut interval = interval(Duration::from_nanos(NANOS_PER_SEC / 30));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut last_tick: i64 = 0;
    let mut fade_tick: i64 = -1;

    let mut context = DrawContext::new()?;

    loop {
        interval.tick().await;

        let tick = Utc::now().timestamp();
        let second = tick % 60;

        if fade_tick >= 0 {
            fade_tick += 1;

            if fade_tick == 30 {
                WALLPAPER.write().await.pop_front();
            } else if fade_tick == 60 {
                fade_tick = -1;
            }
        } else if second == 59 && WALLPAPER.read().await.len() >= 2 {
            fade_tick = 0;
        }

        let alpha = if fade_tick >= 0 {
            ((30 - fade_tick).abs() * 255 / 30).saturating_as()
        } else {
            255
        };

        if tick == last_tick && alpha == 255 {
            continue;
        }

        last_tick = tick;

        if let Err(e) = draw(&mut context, alpha).await {
            error!("Failed to draw: {e:?}");
        }
    }
}

async fn update_worker(mut receiver: mpsc::Receiver<Update>) -> anyhow::Result<()> {
    while let Some(update) = receiver.recv().await {
        match update {
            Update::SwitchBot(data) => *SWITCHBOT.write().await = Some(data),
            Update::OpenWeather(data) => *OPENWEATHER.write().await = Some(data),
            Update::Wallpaper(mut data) => {
                block_in_place(|| draw_frames(&mut data.image));

                let mut lock = WALLPAPER.write().await;
                lock.push_back(data);
            }
        }
    }

    Ok(())
}

fn draw_frames(dst: &mut AlignedImage) {
    const RECT_COLOR: Rgba<u8> = Rgba([0, 0, 0, 180]);

    fill_rect(dst, 50, 50, 880, 750, RECT_COLOR);
    fill_rect(dst, 1380, 50, 2510, 510, RECT_COLOR);
    fill_rect(dst, 50, 1060, 1550, 1550, RECT_COLOR);
    fill_rect(dst, 1600, 860, 2510, 1550, RECT_COLOR);
}

async fn draw(ctx: &mut DrawContext, alpha: u8) -> anyhow::Result<()> {
    const TEXT_COLOR: Color = Color::rgb(255, 255, 255);

    // Draw wallpaper
    if let Some(wallpaper) = WALLPAPER.read().await.front() {
        block_in_place(|| {
            if alpha == 255 {
                ctx.back_buffer.copy_from_slice(&wallpaper.image);
            } else {
                copy_image_with_alpha(&wallpaper.image, &mut ctx.back_buffer, alpha);
            }
        });
    } else {
        block_in_place(|| ctx.back_buffer.fill(0));
    }

    block_in_place(|| {
        // Draw time and date
        let datetime = Local::now();
        let lines = (
            datetime.format("%H").to_compact_string(),
            datetime.format("%M").to_compact_string(),
            datetime.format("%S").to_compact_string(),
        );
        draw_text(ctx, &lines.0, 525, 420, 300.0, TEXT_COLOR, TextAnchor::BottomRight);
        draw_text(ctx, &lines.1, 525, 730, 300.0, TEXT_COLOR, TextAnchor::BottomRight);
        draw_text(ctx, &lines.2, 575, 700, 150.0, TEXT_COLOR, TextAnchor::BottomLeft);

        let lines = (
            datetime.format("%b %e, %Y").to_compact_string().to_ascii_uppercase(),
            datetime.format("%a").to_compact_string().to_ascii_uppercase(),
        );
        draw_text(ctx, &lines.0, 2400, 100, 140.0, TEXT_COLOR, TextAnchor::TopRight);
        draw_text(ctx, &lines.1, 2400, 280, 140.0, TEXT_COLOR, TextAnchor::TopRight);
    });

    // Draw SwitchBot measurements
    if let Some(data) = &*SWITCHBOT.read().await {
        block_in_place(|| {
            let settings::Switchbot { devices, .. } = settings::switchbot();

            let lines = (
                devices.0.name.to_ascii_uppercase(),
                format_compact!("{:.1}", data.indoor.temperature),
                format_compact!("{:}", data.indoor.humidity),
            );
            draw_text(ctx, &lines.0, 160, 1110, 80.0, TEXT_COLOR, TextAnchor::TopLeft);
            draw_text(ctx, &lines.1, 370, 1360, 110.0, TEXT_COLOR, TextAnchor::BottomRight);
            draw_text(ctx, "°C", 410, 1355, 80.0, TEXT_COLOR, TextAnchor::BottomLeft);
            draw_text(ctx, &lines.2, 370, 1500, 110.0, TEXT_COLOR, TextAnchor::BottomRight);
            draw_text(ctx, "%", 430, 1495, 80.0, TEXT_COLOR, TextAnchor::BottomLeft);

            let lines = (
                devices.1.name.to_ascii_uppercase(),
                format_compact!("{:.1}", data.outdoor.temperature),
                format_compact!("{:}", data.outdoor.humidity),
            );
            draw_text(ctx, &lines.0, 640, 1110, 80.0, TEXT_COLOR, TextAnchor::TopLeft);
            draw_text(ctx, &lines.1, 850, 1360, 110.0, TEXT_COLOR, TextAnchor::BottomRight);
            draw_text(ctx, "°C", 890, 1355, 80.0, TEXT_COLOR, TextAnchor::BottomLeft);
            draw_text(ctx, &lines.2, 850, 1500, 110.0, TEXT_COLOR, TextAnchor::BottomRight);
            draw_text(ctx, "%", 910, 1495, 80.0, TEXT_COLOR, TextAnchor::BottomLeft);

            let lines = (
                devices.2.name.to_ascii_uppercase(),
                format_compact!("{:.1}", data.tank.temperature),
                format_compact!("{:}", data.tank.humidity),
            );
            draw_text(ctx, &lines.0, 1110, 1110, 80.0, TEXT_COLOR, TextAnchor::TopLeft);
            draw_text(ctx, &lines.1, 1320, 1360, 110.0, TEXT_COLOR, TextAnchor::BottomRight);
            draw_text(ctx, "°C", 1370, 1355, 80.0, TEXT_COLOR, TextAnchor::BottomLeft);
            draw_text(ctx, &lines.2, 1330, 1500, 110.0, TEXT_COLOR, TextAnchor::BottomRight);
            draw_text(ctx, "%", 1390, 1495, 80.0, TEXT_COLOR, TextAnchor::BottomLeft);
        });
    }

    // Draw OpenWeather measurements
    if let Some(data) = &*OPENWEATHER.read().await {
        block_in_place(|| {
            draw_image(&mut ctx.back_buffer, 1888, 920, &data.icon);

            let lines = (
                data.description.to_ascii_uppercase(),
                format_compact!("{}", WithCommas::from(data.pressure)),
            );
            draw_text(ctx, &lines.0, 2055, 1340, 70.0, TEXT_COLOR, TextAnchor::BottomCenter);
            draw_text(ctx, &lines.1, 2100, 1500, 105.0, TEXT_COLOR, TextAnchor::BottomRight);
            draw_text(ctx, "hPA", 2130, 1495, 75.0, TEXT_COLOR, TextAnchor::BottomLeft);
        });
    }

    // Flip
    block_in_place(|| {
        let mut map = ctx.card.map_dumb_buffer(&mut ctx.dumb_buffer)?;
        map.copy_from_slice(&ctx.back_buffer);

        anyhow::Ok(())
    })?;

    Ok(())
}

fn copy_image_with_alpha(src: &AlignedImage, dst: &mut AlignedImage, alpha: u8) {
    debug_assert!(src.len() == dst.len());

    unsafe {
        // Set up alpha
        let zero = _mm512_setzero_si512();
        let alpha_mask = _mm512_set1_epi32(0xff00_0000_u32.cast_signed());

        let alpha_8x16 = _mm512_set1_epi8(alpha.cast_signed());
        let alpha_16x16 = (
            _mm512_unpacklo_epi8(alpha_8x16, zero),
            _mm512_unpackhi_epi8(alpha_8x16, zero),
        );

        let mut psrc: *const __m512i = src.as_ptr().cast();
        let mut pdst: *mut __m512i = dst.as_mut_ptr().cast();

        for _ in 0..(src.len() / 64) {
            // Load a 16-pixel row from src
            let src_8x16 = _mm512_load_si512(psrc);

            // Unpack each channel to 16bit (lo, hi)
            let src_16x16 = (
                _mm512_unpacklo_epi8(src_8x16, zero),
                _mm512_unpackhi_epi8(src_8x16, zero),
            );

            // dst = (src * a)
            let dst_16x16 = (
                _mm512_mullo_epi16(src_16x16.0, alpha_16x16.0),
                _mm512_mullo_epi16(src_16x16.1, alpha_16x16.1),
            );

            // dst = (dst + (dst >> 8) + 1) >> 8
            let dst_16x16 = (
                _mm512_add_epi16(dst_16x16.0, _mm512_srli_epi16(dst_16x16.0, 8)),
                _mm512_add_epi16(dst_16x16.1, _mm512_srli_epi16(dst_16x16.0, 8)),
            );
            let dst_16x16 = (
                _mm512_add_epi16(dst_16x16.0, _mm512_set1_epi16(1)),
                _mm512_add_epi16(dst_16x16.1, _mm512_set1_epi16(1)),
            );
            let dst_16x16 = (_mm512_srli_epi16(dst_16x16.0, 8), _mm512_srli_epi16(dst_16x16.1, 8));

            // Store the result
            let dst_8x16 = _mm512_packus_epi16(dst_16x16.0, dst_16x16.1);
            let dst_8x16 = _mm512_or_si512(dst_8x16, alpha_mask);

            _mm512_store_si512(pdst, dst_8x16);

            psrc = psrc.add(1);
            pdst = pdst.add(1);
        }
    }
}

fn draw_text(ctx: &mut DrawContext, text: &str, x: u32, y: u32, size: f32, color: Color, anchor: TextAnchor) {
    let metrics = Metrics::new(size, size * 1.2);
    let mut buffer = Buffer::new(&mut ctx.font_system, metrics);
    let mut buffer = buffer.borrow_with(&mut ctx.font_system);

    let attrs = Attrs::new().family(Family::Name("Lato")).weight(Weight::BOLD);
    buffer.set_text(text, &attrs, Shaping::Advanced, Some(Align::Left));

    let width = buffer
        .layout_runs()
        .map(|run| run.line_w.ceil().to_i32().unwrap_or(0))
        .max()
        .unwrap_or(0);
    let height: i32 = buffer
        .layout_runs()
        .map(|run| run.line_height.ceil().to_i32().unwrap_or(0))
        .sum();

    buffer.draw(&mut ctx.swash_cache, color, |bx, by, w, h, color| {
        if w != 1 || h != 1 {
            return;
        }

        let (x, y) = (x.cast_signed(), y.cast_signed());
        let x = match anchor {
            TextAnchor::TopLeft | TextAnchor::BottomLeft => x + bx,
            TextAnchor::TopCenter | TextAnchor::BottomCenter => x - width / 2 + bx,
            TextAnchor::TopRight | TextAnchor::BottomRight => x - width + bx,
        };
        let y = match anchor {
            TextAnchor::TopLeft | TextAnchor::TopCenter | TextAnchor::TopRight => y + by,
            TextAnchor::BottomLeft | TextAnchor::BottomCenter | TextAnchor::BottomRight => y - height + by,
        };
        let (Some(x), Some(y)) = (x.to_u32(), y.to_u32()) else {
            return;
        };

        if let Some(pixel) = ctx.back_buffer.get_pixel_mut_checked(x, y) {
            pixel.blend(&Rgba(color.as_rgba()));
        }
    });
}

fn fill_rect(dst: &mut AlignedImage, l: u32, t: u32, r: u32, b: u32, color: Rgba<u8>) {
    let r = r.min(dst.width());
    let t = t.min(dst.height());
    let (l, r) = (u32::min(l, r), u32::max(l, r));
    let (t, b) = (u32::min(t, b), u32::max(t, b));

    unsafe {
        #[rustfmt::skip]
        let shuffle_mask = _mm512_set_epi8(
            15, 15, 15, 15, 11, 11, 11, 11, 7, 7, 7, 7, 3, 3, 3, 3,
            15, 15, 15, 15, 11, 11, 11, 11, 7, 7, 7, 7, 3, 3, 3, 3,
            15, 15, 15, 15, 11, 11, 11, 11, 7, 7, 7, 7, 3, 3, 3, 3,
            15, 15, 15, 15, 11, 11, 11, 11, 7, 7, 7, 7, 3, 3, 3, 3,
        );
        let alpha_mask = _mm512_set1_epi32(0xff00_0000_u32.cast_signed());
        let zero = _mm512_setzero_si512();

        // Set up src and alpha
        let src_8x16 = _mm512_set1_epi32(i32::from_le_bytes(color.0));
        let src_16x16 = (
            _mm512_unpacklo_epi8(src_8x16, zero),
            _mm512_unpackhi_epi8(src_8x16, zero),
        );

        let alpha_8x16 = _mm512_shuffle_epi8(src_8x16, shuffle_mask);
        let alpha_16x16 = (
            _mm512_unpacklo_epi8(alpha_8x16, zero),
            _mm512_unpackhi_epi8(alpha_8x16, zero),
        );

        for y in t..b {
            let dst_offset = (y * dst.stride() + l.div_ceil(16) * 64) as usize;
            let mut pdst: *mut __m512i = dst.as_mut_ptr().add(dst_offset).cast();

            for _ in l.div_ceil(16)..(r / 16) {
                // Load a 16-pixel row from dst
                let dst_8x16 = _mm512_load_si512(pdst);

                // Unpack each channel to 16bit (lo, hi)
                let dst_16x16 = (
                    _mm512_unpacklo_epi8(dst_8x16, zero),
                    _mm512_unpackhi_epi8(dst_8x16, zero),
                );

                // dst = (src * a + dst * (255 - a))
                let src_16x16 = (
                    _mm512_mullo_epi16(src_16x16.0, alpha_16x16.0),
                    _mm512_mullo_epi16(src_16x16.1, alpha_16x16.1),
                );
                let dst_16x16 = (
                    _mm512_mullo_epi16(dst_16x16.0, _mm512_sub_epi16(_mm512_set1_epi16(255), alpha_16x16.0)),
                    _mm512_mullo_epi16(dst_16x16.1, _mm512_sub_epi16(_mm512_set1_epi16(255), alpha_16x16.1)),
                );
                let dst_16x16 = (
                    _mm512_add_epi16(src_16x16.0, dst_16x16.0),
                    _mm512_add_epi16(src_16x16.1, dst_16x16.1),
                );

                // dst = (dst + (dst >> 8) + 1) >> 8
                let dst_16x16 = (
                    _mm512_add_epi16(dst_16x16.0, _mm512_srli_epi16(dst_16x16.0, 8)),
                    _mm512_add_epi16(dst_16x16.1, _mm512_srli_epi16(dst_16x16.1, 8)),
                );
                let dst_16x16 = (
                    _mm512_add_epi16(dst_16x16.0, _mm512_set1_epi16(1)),
                    _mm512_add_epi16(dst_16x16.1, _mm512_set1_epi16(1)),
                );
                let dst_16x16 = (_mm512_srli_epi16(dst_16x16.0, 8), _mm512_srli_epi16(dst_16x16.1, 8));

                // Store the result
                let dst_8x16 = _mm512_packus_epi16(dst_16x16.0, dst_16x16.1);
                let dst_8x16 = _mm512_or_si512(dst_8x16, alpha_mask);

                _mm512_store_si512(pdst, dst_8x16);

                pdst = pdst.add(1);
            }
        }
    }

    for y in t..b {
        for x in l..(l.div_ceil(16) * 16) {
            dst.get_pixel_mut(x, y).blend(&color);
        }

        for x in (r / 16 * 16)..r {
            dst.get_pixel_mut(x, y).blend(&color);
        }
    }
}

fn draw_image(dst: &mut AlignedImage, x: u32, y: u32, src: &AlignedImage) {
    assert!(x.is_multiple_of(16));
    assert!(src.width().is_multiple_of(16));

    unsafe {
        #[rustfmt::skip]
        let shuffle_mask = _mm512_set_epi8(
            15, 15, 15, 15, 11, 11, 11, 11, 7, 7, 7, 7, 3, 3, 3, 3,
            15, 15, 15, 15, 11, 11, 11, 11, 7, 7, 7, 7, 3, 3, 3, 3,
            15, 15, 15, 15, 11, 11, 11, 11, 7, 7, 7, 7, 3, 3, 3, 3,
            15, 15, 15, 15, 11, 11, 11, 11, 7, 7, 7, 7, 3, 3, 3, 3,
        );
        let alpha_mask = _mm512_set1_epi32(0xff00_0000_u32.cast_signed());
        let zero = _mm512_setzero_si512();

        let mut psrc: *const __m512i = src.as_ptr().cast();

        for src_y in 0..src.height() {
            let dst_offset = ((y + src_y) * dst.stride() + x / 16 * 64) as usize;
            let mut pdst: *mut __m512i = dst.as_mut_ptr().add(dst_offset).cast();

            for _ in 0..src.width() / 16 {
                // Load a 16-pixel row from src
                let src_8x16 = _mm512_load_si512(psrc);

                // Skip if all alpha values are zero
                if _mm512_test_epi32_mask(src_8x16, alpha_mask) == 0 {
                    psrc = psrc.add(1);
                    pdst = pdst.add(1);

                    continue;
                }

                // Load a 16-pixel row from dst
                let dst_8x16 = _mm512_load_si512(pdst);

                // Unpack each channel to 16bit (lo, hi)
                let src_16x16 = (
                    _mm512_unpacklo_epi8(src_8x16, zero),
                    _mm512_unpackhi_epi8(src_8x16, zero),
                );
                let dst_16x16 = (
                    _mm512_unpacklo_epi8(dst_8x16, zero),
                    _mm512_unpackhi_epi8(dst_8x16, zero),
                );

                // Unpack alpha value
                let alpha_8x16 = _mm512_shuffle_epi8(src_8x16, shuffle_mask);
                let alpha_16x16 = (
                    _mm512_unpacklo_epi8(alpha_8x16, zero),
                    _mm512_unpackhi_epi8(alpha_8x16, zero),
                );

                // dst = (src * a + dst * (255 - a))
                let src_16x16 = (
                    _mm512_mullo_epi16(src_16x16.0, alpha_16x16.0),
                    _mm512_mullo_epi16(src_16x16.1, alpha_16x16.1),
                );
                let dst_16x16 = (
                    _mm512_mullo_epi16(dst_16x16.0, _mm512_sub_epi16(_mm512_set1_epi16(255), alpha_16x16.0)),
                    _mm512_mullo_epi16(dst_16x16.1, _mm512_sub_epi16(_mm512_set1_epi16(255), alpha_16x16.1)),
                );
                let dst_16x16 = (
                    _mm512_add_epi16(src_16x16.0, dst_16x16.0),
                    _mm512_add_epi16(src_16x16.1, dst_16x16.1),
                );

                // dst = (dst + (dst >> 8) + 1) >> 8
                let dst_16x16 = (
                    _mm512_add_epi16(dst_16x16.0, _mm512_srli_epi16(dst_16x16.0, 8)),
                    _mm512_add_epi16(dst_16x16.1, _mm512_srli_epi16(dst_16x16.1, 8)),
                );
                let dst_16x16 = (
                    _mm512_add_epi16(dst_16x16.0, _mm512_set1_epi16(1)),
                    _mm512_add_epi16(dst_16x16.1, _mm512_set1_epi16(1)),
                );
                let dst_16x16 = (_mm512_srli_epi16(dst_16x16.0, 8), _mm512_srli_epi16(dst_16x16.1, 8));

                // Store the result
                let dst_8x16 = _mm512_packus_epi16(dst_16x16.0, dst_16x16.1);
                let dst_8x16 = _mm512_or_si512(dst_8x16, alpha_mask);

                _mm512_store_si512(pdst, dst_8x16);

                psrc = psrc.add(1);
                pdst = pdst.add(1);
            }
        }
    }
}
