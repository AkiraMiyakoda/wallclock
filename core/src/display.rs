// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

#[allow(clippy::wildcard_imports)]
use std::arch::x86_64::*;
use std::borrow::Cow;
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
use chrono::Timelike;
use chrono::Utc;
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
// `Rgba<u8>` is used only as a convenient 4-byte pixel container.
// The bytes are interpreted as BGRA/XRGB when copied to the DRM framebuffer.
use image::Rgba;
use log::error;
use num_traits::ToPrimitive;
use tokio::task::block_in_place;
use tokio::time::MissedTickBehavior;
use tokio::time::interval;

use crate::SCREEN_DIMENSIONS;
use crate::balbird;
use crate::image::AlignedImage;
use crate::openweather;
use crate::settings;
use crate::switchbot;
use crate::wallpaper;

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
    text_buffer: Buffer,
}

impl Drop for DrawContext {
    fn drop(&mut self) {
        let _ = self.card.destroy_framebuffer(self.frame_buffer);
        let _ = self.card.destroy_dumb_buffer(self.dumb_buffer);
    }
}

impl DrawContext {
    fn new() -> anyhow::Result<Self> {
        // Open the DRM device.
        let settings::Drm { device } = settings::drm();
        let card = Card(OpenOptions::new().read(true).write(true).open(device)?);

        // Get DRM resources.
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
            .find(|crtc| {
                crtc.mode()
                    .is_some_and(|mode| mode.size() == SCREEN_DIMENSIONS.into())
            })
            .or_else(|| {
                resources
                    .crtcs()
                    .iter()
                    .flat_map(|crtc| card.get_crtc(*crtc))
                    .next()
            })
            .ok_or(anyhow!("CRTC not found"))?;

        // Create the framebuffer.
        let dumb_buffer = card.create_dumb_buffer(SCREEN_DIMENSIONS.into(), DrmFourcc::Xrgb8888, 32)?;
        let frame_buffer = card.add_framebuffer(&dumb_buffer, 32, 32)?;

        card.set_crtc(
            crtc.handle(),
            Some(frame_buffer),
            (0, 0),
            &[connector.handle()],
            Some(mode),
        )?;

        let fonts = [Source::Binary(Arc::new(include_bytes!("../fonts/Lato-Bold.ttf")))];
        let mut font_system = FontSystem::new_with_fonts(fonts);
        let text_buffer = Buffer::new(&mut font_system, Metrics::new(16.0, 16.0 * 1.2));

        let (screen_width, screen_height) = SCREEN_DIMENSIONS.into();
        Ok(Self {
            card,
            dumb_buffer,
            frame_buffer,
            back_buffer: AlignedImage::new(screen_width, screen_height),
            font_system,
            swash_cache: SwashCache::new(),
            text_buffer,
        })
    }
}

#[derive(Debug)]
struct SwitchBotLabels {
    indoor: String,
    outdoor: String,
    tank: String,
}

static LABELS: LazyLock<SwitchBotLabels> = LazyLock::new(|| {
    let settings::SwitchBot { devices, .. } = settings::switchbot();
    SwitchBotLabels {
        indoor: devices.indoor.label.to_ascii_uppercase(),
        outdoor: devices.outdoor.label.to_ascii_uppercase(),
        tank: devices.tank.label.to_ascii_uppercase(),
    }
});

pub async fn worker() -> anyhow::Result<()> {
    const NANOS_PER_SEC: u64 = 1_000_000_000;

    let mut interval = interval(Duration::from_nanos(NANOS_PER_SEC / 30));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut last_second: Option<u32> = None;
    let mut context = DrawContext::new()?;

    loop {
        interval.tick().await;

        let now = Utc::now();
        let minute = now.minute();
        let second = now.second();
        let sub_second: i64 = now.timestamp_subsec_millis().into();

        #[allow(clippy::manual_is_multiple_of)]
        let cycle_start = minute % 5 == 0 && second == 0;
        let cycle_end = minute % 5 == 4 && second == 59;

        // Advance the wallpaper once at the start of each 5-minute cycle.
        if cycle_start && Some(second) != last_second {
            wallpaper::move_next().await;
        }

        let mut alpha: u8 = 255;

        if cycle_start {
            // Fade in after each 5-minute cycle.
            alpha = (sub_second * 255 / 1000).saturating_as();
        } else if cycle_end {
            // Fade out before each 5-minute cycle.
            // If no next wallpaper is ready, the same one fades back in.
            alpha = 255 - (sub_second * 255 / 1000).saturating_as::<u8>();
        } else if Some(second) == last_second {
            // Draw a frame once a second if not fading.
            continue;
        }

        last_second = Some(second);

        if let Err(e) = draw(&mut context, alpha).await {
            error!("Failed to draw: {e:?}");
        }
    }
}

async fn draw(ctx: &mut DrawContext, alpha: u8) -> anyhow::Result<()> {
    const RECT_COLOR: Rgba<u8> = Rgba([0, 0, 0, 180]);
    const TEXT_COLOR: Color = Color::rgb(255, 255, 255);

    let wallpaper = wallpaper::get_current().await;
    let switchbot = switchbot::get_latest();
    let openweather = openweather::get_latest();
    let balbird = balbird::get_latest();

    block_in_place(|| {
        // Draw wallpaper
        if let Some(wallpaper) = wallpaper {
            if alpha == 255 {
                ctx.back_buffer.copy_from_slice(&wallpaper.image);
            } else {
                copy_image_with_alpha(&wallpaper.image, &mut ctx.back_buffer, alpha);
            }
        } else {
            ctx.back_buffer.fill(0);
        }

        // Draw background frames
        fill_rect(&mut ctx.back_buffer, 50, 50, 880, 750, RECT_COLOR);
        fill_rect(&mut ctx.back_buffer, 1530, 50, 2510, 270, RECT_COLOR);
        fill_rect(&mut ctx.back_buffer, 50, 1060, 1550, 1550, RECT_COLOR);
        fill_rect(&mut ctx.back_buffer, 1730, 320, 2510, 810, RECT_COLOR);
        fill_rect(&mut ctx.back_buffer, 1600, 860, 2510, 1550, RECT_COLOR);

        // Draw time and date
        let datetime = Local::now();
        let lines = (
            datetime.format("%H").to_string(),
            datetime.format("%M").to_string(),
            datetime.format("%S").to_string(),
        );
        draw_text(ctx, &lines.0, 525, 420, 300.0, TEXT_COLOR, TextAnchor::BottomRight);
        draw_text(ctx, &lines.1, 525, 730, 300.0, TEXT_COLOR, TextAnchor::BottomRight);
        draw_text(ctx, &lines.2, 575, 700, 150.0, TEXT_COLOR, TextAnchor::BottomLeft);

        let text = datetime
            .format("%a, %b %e, %Y")
            .to_string()
            .to_ascii_uppercase();
        draw_text(ctx, &text, 2400, 100, 90.0, TEXT_COLOR, TextAnchor::TopRight);

        // Draw SwitchBot measurements
        if let Some(data) = switchbot {
            let lines = (
                format!("{:.1}", data.indoor.temperature),
                format!("{:.0}", data.indoor.humidity),
            );
            draw_text(ctx, &LABELS.indoor, 160, 1110, 80.0, TEXT_COLOR, TextAnchor::TopLeft);
            draw_text(ctx, &lines.0, 370, 1360, 110.0, TEXT_COLOR, TextAnchor::BottomRight);
            draw_text(ctx, "°C", 410, 1355, 80.0, TEXT_COLOR, TextAnchor::BottomLeft);
            draw_text(ctx, &lines.1, 370, 1500, 110.0, TEXT_COLOR, TextAnchor::BottomRight);
            draw_text(ctx, "%", 430, 1495, 80.0, TEXT_COLOR, TextAnchor::BottomLeft);

            let lines = (
                format!("{:.1}", data.outdoor.temperature),
                format!("{:.0}", data.outdoor.humidity),
            );
            draw_text(ctx, &LABELS.outdoor, 640, 1110, 80.0, TEXT_COLOR, TextAnchor::TopLeft);
            draw_text(ctx, &lines.0, 850, 1360, 110.0, TEXT_COLOR, TextAnchor::BottomRight);
            draw_text(ctx, "°C", 890, 1355, 80.0, TEXT_COLOR, TextAnchor::BottomLeft);
            draw_text(ctx, &lines.1, 850, 1500, 110.0, TEXT_COLOR, TextAnchor::BottomRight);
            draw_text(ctx, "%", 910, 1495, 80.0, TEXT_COLOR, TextAnchor::BottomLeft);

            let lines = (
                format!("{:.1}", data.tank.temperature),
                format!("{:.0}", data.tank.humidity),
            );
            draw_text(ctx, &LABELS.tank, 1110, 1110, 80.0, TEXT_COLOR, TextAnchor::TopLeft);
            draw_text(ctx, &lines.0, 1320, 1360, 110.0, TEXT_COLOR, TextAnchor::BottomRight);
            draw_text(ctx, "°C", 1370, 1355, 80.0, TEXT_COLOR, TextAnchor::BottomLeft);
            draw_text(ctx, &lines.1, 1330, 1500, 110.0, TEXT_COLOR, TextAnchor::BottomRight);
            draw_text(ctx, "%", 1390, 1495, 80.0, TEXT_COLOR, TextAnchor::BottomLeft);
        }

        // Draw Balbird server status
        let health = match &balbird {
            Some(data) if data.is_healthy => "HEALTHY",
            Some(_) => "STALE",
            None => "OFFLINE",
        };
        draw_text(ctx, "SERVER", 1830, 380, 60.0, TEXT_COLOR, TextAnchor::TopLeft);
        draw_text(ctx, health, 2400, 380, 60.0, TEXT_COLOR, TextAnchor::TopRight);

        if let Some(data) = balbird {
            let lines = (
                format_percent(data.memory_usage_percent),
                format_percent(data.swap_usage_percent),
                format_percent(data.disk_usage_percent),
            );

            draw_text(ctx, "MEM", 1830, 480, 60.0, TEXT_COLOR, TextAnchor::TopLeft);
            draw_text(ctx, &lines.0, 2400, 480, 60.0, TEXT_COLOR, TextAnchor::TopRight);
            draw_text(ctx, "SWAP", 1830, 580, 60.0, TEXT_COLOR, TextAnchor::TopLeft);
            draw_text(ctx, &lines.1, 2400, 580, 60.0, TEXT_COLOR, TextAnchor::TopRight);
            draw_text(ctx, "DISK", 1830, 680, 60.0, TEXT_COLOR, TextAnchor::TopLeft);
            draw_text(ctx, &lines.2, 2400, 680, 60.0, TEXT_COLOR, TextAnchor::TopRight);
        }

        // Draw OpenWeather measurements
        if let Some(data) = openweather {
            draw_image(&mut ctx.back_buffer, 1888, 920, &data.icon);

            let lines = (
                data.description.to_ascii_uppercase(),
                format!("{}", WithCommas::primitive(data.pressure)),
            );
            draw_text(ctx, &lines.0, 2055, 1340, 70.0, TEXT_COLOR, TextAnchor::BottomCenter);
            draw_text(ctx, &lines.1, 2100, 1500, 105.0, TEXT_COLOR, TextAnchor::BottomRight);
            draw_text(ctx, "hPa", 2130, 1495, 75.0, TEXT_COLOR, TextAnchor::BottomLeft);
        }

        let mut map = ctx.card.map_dumb_buffer(&mut ctx.dumb_buffer)?;
        map.copy_from_slice(&ctx.back_buffer);

        anyhow::Ok(())
    })?;

    Ok(())
}

fn format_percent<'a>(percent: Option<f64>) -> Cow<'a, str> {
    match percent {
        Some(percent) => format!("{percent:.1} %").into(),
        None => "-- %".into(),
    }
}

fn copy_image_with_alpha(src: &AlignedImage, dst: &mut AlignedImage, alpha: u8) {
    assert_eq!(src.len(), dst.len());
    assert!(src.len().is_multiple_of(64));
    assert!(src.as_ptr().addr().is_multiple_of(64));
    assert!(dst.as_ptr().addr().is_multiple_of(64));

    // SAFETY: This program is only supported on Zen 4 systems.
    // Both buffers are 64-byte aligned, have the same length,
    // and the length is a multiple of 64 bytes.
    unsafe {
        copy_image_with_alpha_avx512(src.as_ref(), dst.as_mut(), alpha);
    }
}

fn draw_text(ctx: &mut DrawContext, text: &str, x: u32, y: u32, size: f32, color: Color, anchor: TextAnchor) {
    let mut buffer = ctx.text_buffer.borrow_with(&mut ctx.font_system);
    buffer.set_metrics(Metrics::new(size, size * 1.2));

    let attrs = Attrs::new()
        .family(Family::Name("Lato"))
        .weight(Weight::BOLD);
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
    let (l, r) = (l.min(r), l.max(r));
    let (t, b) = (t.min(b), t.max(b));

    let l = l.min(dst.width());
    let r = r.min(dst.width());
    let t = t.min(dst.height());
    let b = b.min(dst.height());

    if l == r || t == b {
        return;
    }

    let simd_l = l.div_ceil(16) * 16;
    let simd_r = r / 16 * 16;

    let left_end = simd_l.min(r);
    let right_start = simd_r.max(left_end);

    if simd_l < simd_r {
        // SAFETY: `AlignedImage` is 64-byte aligned, rows are 16-pixel aligned,
        // and `simd_l..simd_r` is inside the image and aligned to 16 pixels.
        unsafe {
            fill_rect_avx512(dst, simd_l, t, simd_r, b, color);
        }
    }

    fill_rect_scalar(dst, l, t, left_end, b, color);
    fill_rect_scalar(dst, right_start, t, r, b, color);
}

fn fill_rect_scalar(dst: &mut AlignedImage, l: u32, t: u32, r: u32, b: u32, color: Rgba<u8>) {
    for y in t..b {
        for x in l..r {
            dst.get_pixel_mut(x, y).blend(&color);
        }
    }
}

fn draw_image(dst: &mut AlignedImage, x: u32, y: u32, src: &AlignedImage) {
    assert!(x.is_multiple_of(16));
    assert!(src.width().is_multiple_of(16));
    assert!(src.as_ptr().addr().is_multiple_of(64));
    assert!(dst.as_ptr().addr().is_multiple_of(64));
    assert!(x.checked_add(src.width()).is_some_and(|r| r <= dst.width()));
    assert!(
        y.checked_add(src.height())
            .is_some_and(|b| b <= dst.height())
    );

    // SAFETY: Both images are 64-byte aligned. `x` and `src.width()` are
    // multiples of 16 pixels, and the source image fits inside the destination.
    unsafe {
        draw_image_avx512(dst, x, y, src);
    }
}

#[allow(dead_code)]
#[allow(clippy::cast_precision_loss)]
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];

    let mut size = bytes as f64;
    let mut unit = 0;

    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// # Safety
///
/// `src` and `dst` must be 64-byte aligned, have the same length,
/// and their length must be a multiple of 64.
/// The CPU must support AVX-512F and AVX-512BW.
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn copy_image_with_alpha_avx512(src: &[u8], dst: &mut [u8], alpha: u8) {
    debug_assert_eq!(src.len(), dst.len());
    debug_assert!(src.len().is_multiple_of(64));
    debug_assert!(src.as_ptr().addr().is_multiple_of(64));
    debug_assert!(dst.as_ptr().addr().is_multiple_of(64));

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

            // dst = src * a / 255
            let dst_16x16 = (
                _mm512_mullo_epi16(src_16x16.0, alpha_16x16.0),
                _mm512_mullo_epi16(src_16x16.1, alpha_16x16.1),
            );
            let dst_16x16 = div_255_avx512(dst_16x16);

            // Store the result
            let dst_8x16 = _mm512_packus_epi16(dst_16x16.0, dst_16x16.1);
            let dst_8x16 = _mm512_or_si512(dst_8x16, alpha_mask);

            _mm512_store_si512(pdst, dst_8x16);

            psrc = psrc.add(1);
            pdst = pdst.add(1);
        }
    }
}

/// # Safety
///
/// `dst` must be 64-byte aligned.
/// `l` and `r` must be multiples of 16.
/// The rectangle must be inside `dst`.
/// The CPU must support AVX-512F and AVX-512BW.
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn fill_rect_avx512(dst: &mut AlignedImage, l: u32, t: u32, r: u32, b: u32, color: Rgba<u8>) {
    debug_assert!(dst.as_ptr().addr().is_multiple_of(64));
    debug_assert!(l.is_multiple_of(16));
    debug_assert!(r.is_multiple_of(16));
    debug_assert!(l <= r);
    debug_assert!(t <= b);
    debug_assert!(r <= dst.width());
    debug_assert!(b <= dst.height());

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
            let dst_offset = (y * dst.stride() + l * 4) as usize;
            let mut pdst: *mut __m512i = dst.as_mut_ptr().add(dst_offset).cast();

            for _ in (l / 16)..(r / 16) {
                // Load a 16-pixel row from dst
                let dst_8x16 = _mm512_load_si512(pdst);

                // Unpack each channel to 16bit (lo, hi)
                let dst_16x16 = (
                    _mm512_unpacklo_epi8(dst_8x16, zero),
                    _mm512_unpackhi_epi8(dst_8x16, zero),
                );

                let dst_16x16 = blend_src_over_dst_avx512(src_16x16, dst_16x16, alpha_16x16);

                // Store the result
                let dst_8x16 = _mm512_packus_epi16(dst_16x16.0, dst_16x16.1);
                let dst_8x16 = _mm512_or_si512(dst_8x16, alpha_mask);

                _mm512_store_si512(pdst, dst_8x16);

                pdst = pdst.add(1);
            }
        }
    }
}

/// # Safety
///
/// `src` and `dst` must be 64-byte aligned.
/// `x` and `src.width()` must be multiples of 16.
/// The source image must fit inside `dst` at `(x, y)`.
/// The CPU must support AVX-512F and AVX-512BW.
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn draw_image_avx512(dst: &mut AlignedImage, x: u32, y: u32, src: &AlignedImage) {
    debug_assert!(x.is_multiple_of(16));
    debug_assert!(src.width().is_multiple_of(16));
    debug_assert!(src.as_ptr().addr().is_multiple_of(64));
    debug_assert!(dst.as_ptr().addr().is_multiple_of(64));
    debug_assert!(x.checked_add(src.width()).is_some_and(|r| r <= dst.width()));
    debug_assert!(
        y.checked_add(src.height())
            .is_some_and(|b| b <= dst.height())
    );

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

                let dst_16x16 = blend_src_over_dst_avx512(src_16x16, dst_16x16, alpha_16x16);

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

// Blend 16 pixels with source-over alpha:
// dst = (src * a + dst * (255 - a)) / 255.
#[inline]
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn blend_src_over_dst_avx512(
    src_16x16: (__m512i, __m512i), dst_16x16: (__m512i, __m512i), alpha_16x16: (__m512i, __m512i),
) -> (__m512i, __m512i) {
    unsafe {
        let inv_alpha_16x16 = (
            _mm512_sub_epi16(_mm512_set1_epi16(255), alpha_16x16.0),
            _mm512_sub_epi16(_mm512_set1_epi16(255), alpha_16x16.1),
        );

        let src_16x16 = (
            _mm512_mullo_epi16(src_16x16.0, alpha_16x16.0),
            _mm512_mullo_epi16(src_16x16.1, alpha_16x16.1),
        );
        let dst_16x16 = (
            _mm512_mullo_epi16(dst_16x16.0, inv_alpha_16x16.0),
            _mm512_mullo_epi16(dst_16x16.1, inv_alpha_16x16.1),
        );
        let dst_16x16 = (
            _mm512_add_epi16(src_16x16.0, dst_16x16.0),
            _mm512_add_epi16(src_16x16.1, dst_16x16.1),
        );

        div_255_avx512(dst_16x16)
    }
}

// Divide 16-bit color channels by 255 with rounding.
// This is used after alpha multiplication.
#[inline]
#[target_feature(enable = "avx512f,avx512bw")]
unsafe fn div_255_avx512(v: (__m512i, __m512i)) -> (__m512i, __m512i) {
    // (x + (x >> 8) + 1) >> 8
    let v = (
        _mm512_add_epi16(v.0, _mm512_srli_epi16(v.0, 8)),
        _mm512_add_epi16(v.1, _mm512_srli_epi16(v.1, 8)),
    );
    let v = (
        _mm512_add_epi16(v.0, _mm512_set1_epi16(1)),
        _mm512_add_epi16(v.1, _mm512_set1_epi16(1)),
    );

    (_mm512_srli_epi16(v.0, 8), _mm512_srli_epi16(v.1, 8))
}
