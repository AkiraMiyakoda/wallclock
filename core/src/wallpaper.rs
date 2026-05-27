// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::time::Duration;

use anyhow::anyhow;
use chrono::TimeDelta;
use chrono::Utc;
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

#[derive(Debug, Deserialize)]
struct Message {
    images: Vec<ImageRow>,
}

#[derive(Debug, Deserialize)]
struct ImageRow {
    urlbase: String,
}

#[derive(Debug)]
pub struct WallpaperData(Vec<u8>);

impl AsRef<Vec<u8>> for WallpaperData {
    fn as_ref(&self) -> &Vec<u8> {
        &self.0
    }
}

pub async fn worker(sender: &mpsc::Sender<Update>) -> anyhow::Result<()> {
    let mut interval = interval(Duration::from_mins(1));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut last_tick: i64 = 0;

    loop {
        interval.tick().await;

        let tick = Utc::now().timestamp() / TimeDelta::hours(3).num_seconds();
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
    const METADATA_URL: &str = "https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=1&mkt=ja-JP";
    const BASE_URL: &str = "https://www.bing.com/";

    // Get the metadata of Bing's picture of the day
    let msg: Message = REST_CLIENT.get(METADATA_URL).send().await?.json().await?;
    let Some(row) = msg.images.into_iter().next() else {
        return Err(anyhow!("Metadata is empty"));
    };

    // Get the picture
    let url = Url::parse(BASE_URL)?;
    let url = url.join(&format!("{}_UHD.jpg", row.urlbase))?;
    let data = REST_CLIENT.get(url).send().await?.bytes().await?;

    // Decode and resize the picture
    block_in_place(|| {
        let (width, height) = SCREEN_DIMENSIONS;

        Ok(WallpaperData(
            image::load_from_memory(&data)?
                .resize_to_fill(width, height, FilterType::Lanczos3)
                .into_rgba8()
                .into_raw_bgra(),
        ))
    })
}
