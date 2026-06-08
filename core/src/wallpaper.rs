// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::sync::LazyLock;
use std::time::Duration;

use chrono::TimeDelta;
use chrono::Utc;
use image::imageops::FilterType;
use log::error;
use log::info;
use reqwest::Url;
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::task::block_in_place;
use tokio::time::MissedTickBehavior;
use tokio::time::interval;

use crate::REST_CLIENT;
use crate::SCREEN_DIMENSIONS;
use crate::Update;
use crate::alphablend::AlignedRgbaImage;

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

static URL_QUEUE: LazyLock<Mutex<Vec<String>>> = LazyLock::new(|| Mutex::new(vec![]));

async fn inquire() -> anyhow::Result<WallpaperData> {
    const API_URL: &str = "https://bing.com/HPImageArchive.aspx?format=js&idx=0&n=8&mkt=ja-JP";
    const BASE_URL: &str = "https://bing.com/";

    let url = {
        let mut queue = URL_QUEUE.lock().await;

        if queue.is_empty() {
            // Get list of pictures from Bing
            let res = REST_CLIENT.get(API_URL).send().await?;
            let msg: Message = res.json().await?;

            let base_url = Url::parse(BASE_URL)?;
            *queue = msg
                .images
                .into_iter()
                .flat_map(|image| {
                    base_url
                        .join(&format!("{}_UHD.jpg", image.urlbase))
                        .map(|url| url.to_string())
                })
                .collect();
        }

        queue.pop().expect("Queue is empty")
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
