// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::sync::LazyLock;
use std::time::Duration;

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
use tokio::sync::mpsc;
use tokio::task::block_in_place;
use tokio::time::MissedTickBehavior;
use tokio::time::interval;

use crate::REST_CLIENT;
use crate::SCREEN_DIMENSIONS;
use crate::Update;
use crate::image::AlignedImage;

#[derive(Debug, Deserialize)]
struct Message {
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

pub async fn worker(sender: mpsc::Sender<Update>) -> anyhow::Result<()> {
    let mut interval = interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut last_tick: i64 = -1;

    loop {
        interval.tick().await;

        let tick = (Utc::now().timestamp() + 10) / TimeDelta::minutes(5).num_seconds();
        if tick == last_tick {
            continue;
        }

        match inquire().await {
            Ok(data) => {
                info!("Wallpaper updated");
                sender.send(Update::Wallpaper(data)).await?;
            }
            Err(e) => {
                error!("Failed to update wallpaper: {e:?}");
            }
        }

        last_tick = tick;
    }
}

static URL_QUEUE: LazyLock<RwLock<Vec<String>>> = LazyLock::new(|| RwLock::new(vec![]));

async fn inquire() -> anyhow::Result<WallpaperData> {
    const API_URL: &str = "https://bing.com/HPImageArchive.aspx?format=js&idx=0&n=8&mkt=ja-JP";
    const BASE_URL: &str = "https://bing.com/";

    let urls = if URL_QUEUE.read().await.is_empty() {
        // Get list of pictures from Bing
        let res = REST_CLIENT.get(API_URL).send().await?;
        let msg: Message = res.json().await?;

        let base_url = Url::parse(BASE_URL)?;
        let urls = msg
            .images
            .into_iter()
            .flat_map(|image| {
                base_url
                    .join(&format!("{}_UHD.jpg", image.urlbase))
                    .map(|url| url.to_string())
            })
            .collect_vec();

        Some(urls)
    } else {
        None
    };
    let url = {
        let mut queue = URL_QUEUE.write().await;
        if let Some(urls) = urls
            && queue.is_empty()
        {
            *queue = urls;
        }
        let Some(url) = queue.pop() else {
            bail!("Bing API returned an empty image array");
        };

        url
    };

    // Get the picture
    let res = REST_CLIENT.get(&url).send().await?;
    let data = res.bytes().await?;

    // Decode and resize the picture
    let image = block_in_place(|| {
        let (width, height) = SCREEN_DIMENSIONS.into();
        let image = image::load_from_memory(&data)?;
        let image = image.resize_to_fill(width, height, FilterType::Lanczos3);

        anyhow::Ok(image.into())
    })?;

    Ok(WallpaperData { image })
}
