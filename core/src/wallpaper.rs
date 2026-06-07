// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::time::Duration;

use anyhow::bail;
use chrono::TimeDelta;
use chrono::Utc;
use compact_str::format_compact;
use image::imageops::FilterType;
use log::error;
use log::info;
use reqwest::Url;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::task::block_in_place;
use tokio::time::MissedTickBehavior;
use tokio::time::interval;

use crate::REST_CLIENT;
use crate::SCREEN_DIMENSIONS;
use crate::Update;
use crate::alphablend::AlignedRgbaImage;
use crate::settings;

#[derive(Debug, Deserialize)]
struct Message {
    data: Vec<DataRow>,
}

#[derive(Debug, Deserialize)]
struct DataRow {
    path: String,
}

#[derive(Debug)]
pub struct WallpaperData {
    pub image: AlignedRgbaImage,
}

pub async fn worker(sender: &mpsc::Sender<Update>) -> anyhow::Result<()> {
    let mut interval = interval(Duration::from_mins(1));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut last_tick: i64 = 0;

    loop {
        interval.tick().await;

        let tick = (Utc::now().timestamp() - 20) / TimeDelta::minutes(3).num_seconds();
        if tick == last_tick {
            continue;
        }

        match inquire().await {
            Ok(data) => {
                info!("Wallpaper updated");

                last_tick = tick;
                sender.send(Update::Wallpaper(data)).await?;
            }
            Err(e) => {
                error!("Failed to update wallpaper: {e:?}");
            }
        }
    }
}

async fn inquire() -> anyhow::Result<WallpaperData> {
    const BASE_URL: &str = "https://wallhaven.cc/api/v1/search";

    let settings::Wallhaven {
        query,
        categories,
        purity,
    } = settings::wallhaven();

    // Get random picture info from Wallhaven
    let atleast = format_compact!("{}x{}", SCREEN_DIMENSIONS.0, SCREEN_DIMENSIONS.1);
    let params: [(&str, &str); _] = [
        ("q", &query),
        ("categories", &categories),
        ("purity", &purity),
        ("atleast", &atleast),
        ("sorting", "random"),
    ];
    let url = Url::parse_with_params(BASE_URL, params)?;
    let res = REST_CLIENT.get(url).send().await?;
    let msg: Message = res.json().await?;
    let Some(row) = msg.data.into_iter().next() else {
        bail!("Metadata is empty");
    };

    // Get the picture
    let res = REST_CLIENT.get(row.path).send().await?;
    let data = res.bytes().await?;

    // Decode and resize the picture
    block_in_place(|| {
        let (width, height) = SCREEN_DIMENSIONS.into();
        let image = image::load_from_memory(&data)?;
        let image = image.resize_to_fill(width, height, FilterType::Lanczos3);
        let image: AlignedRgbaImage = image.into();

        Ok(WallpaperData { image })
    })
}
