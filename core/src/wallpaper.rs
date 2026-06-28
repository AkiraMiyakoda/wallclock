// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::Context;
use anyhow::bail;
use chrono::TimeDelta;
use chrono::Utc;
use image::imageops::FilterType;
use itertools::Itertools;
use log::error;
use log::info;
use reqwest::Url;
use serde::Deserialize;
use tokio::sync::RwLock;
use tokio::task::spawn_blocking;
use tokio::time::MissedTickBehavior;
use tokio::time::interval;

use crate::REST_CLIENT;
use crate::SCREEN_DIMENSIONS;
use crate::image::AlignedImage;

#[derive(Debug, Deserialize)]
struct BingResponse {
    images: Vec<Image>,
}

#[derive(Debug, Deserialize)]
struct Image {
    urlbase: String,
}

#[derive(Debug)]
pub struct WallpaperData {
    pub image: AlignedImage,
}

static DATA_QUEUE: LazyLock<RwLock<VecDeque<Arc<WallpaperData>>>> = LazyLock::new(|| RwLock::new(VecDeque::new()));

pub async fn get_current() -> Option<Arc<WallpaperData>> {
    let queue = DATA_QUEUE.read().await;
    queue.front().cloned()
}

pub async fn move_next() -> bool {
    let mut queue = DATA_QUEUE.write().await;
    if queue.len() >= 2 {
        queue.pop_front();
        true
    } else {
        false
    }
}

pub async fn worker() -> anyhow::Result<()> {
    const MAX_QUEUE_SIZE: usize = 2;

    let mut interval = interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut last_tick: i64 = -1;

    loop {
        interval.tick().await;

        // Run about 10 seconds before each 5-minute boundary,
        // so this worker does not run at the same time as the others.
        let tick = (Utc::now().timestamp() + 10) / TimeDelta::minutes(5).num_seconds();
        if tick == last_tick {
            continue;
        }

        {
            let queue = DATA_QUEUE.read().await;
            if queue.len() >= MAX_QUEUE_SIZE {
                last_tick = tick;
                continue;
            }
        }

        match inquire().await {
            Ok(data) => {
                let data = Arc::new(data);
                let mut queue = DATA_QUEUE.write().await;
                if queue.len() < MAX_QUEUE_SIZE {
                    queue.push_back(data);
                    info!("Wallpaper updated");
                }
            }
            Err(e) => {
                error!("Failed to update wallpaper: {e:?}");
            }
        }

        // Update this even on failure to avoid retrying every second.
        last_tick = tick;
    }
}

static URL_QUEUE: LazyLock<RwLock<VecDeque<String>>> = LazyLock::new(|| RwLock::new(VecDeque::new()));

async fn inquire() -> anyhow::Result<WallpaperData> {
    const API_URL: &str = "https://bing.com/HPImageArchive.aspx?format=js&idx=0&n=8&mkt=ja-JP";
    const BASE_URL: &str = "https://bing.com/";

    // Fetch the latest URL list before locking the queue, so the lock is held briefly.
    let response: BingResponse = REST_CLIENT
        .get(API_URL)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let base_url = Url::parse(BASE_URL)?;
    let urls = response
        .images
        .into_iter()
        .map(|image| base_url.join(&format!("{}_UHD.jpg", image.urlbase)))
        .map_ok(|url| url.to_string())
        .collect::<Result<VecDeque<_>, _>>()
        .with_context(|| "Bing API returned malformed URLs")?;

    let url = {
        let mut queue = URL_QUEUE.write().await;

        // Use the new URL list only when the queue is empty.
        // This keeps the current rotation stable while still refreshing the list often.
        if queue.is_empty() {
            *queue = urls;
        }

        let Some(url) = queue.pop_front() else {
            bail!("Bing API returned an empty list");
        };

        url
    };

    let data = REST_CLIENT
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    // Decode and resize the picture on Tokio's blocking thread.
    let image = spawn_blocking(move || {
        let (width, height) = SCREEN_DIMENSIONS.into();
        let image = image::load_from_memory(&data)?;
        let image = image.resize_to_fill(width, height, FilterType::Lanczos3);

        anyhow::Ok(image.into())
    })
    .await??;

    Ok(WallpaperData { image })
}
