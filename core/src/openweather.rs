// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::time::Duration;

use anyhow::bail;
use chrono::TimeDelta;
use chrono::Utc;
use compact_str::CompactString;
use compact_str::ToCompactString;
use image::RgbaImage;
use log::error;
use log::info;
use reqwest::Url;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::task::block_in_place;
use tokio::time::MissedTickBehavior;
use tokio::time::interval;

use crate::REST_CLIENT;
use crate::Update;
use crate::image::IntoBgra8;
use crate::settings;

#[derive(Debug, Deserialize)]
struct Message {
    weather: Vec<WeatherRow>,
    main: MainRow,
}

#[derive(Debug, Deserialize)]
struct WeatherRow {
    icon: CompactString,
    description: CompactString,
}

#[derive(Debug, Deserialize)]
struct MainRow {
    pressure: i32,
}

#[derive(Debug)]
pub struct OpenWeatherData {
    pub icon: RgbaImage,
    pub description: CompactString,
    pub pressure: i32,
}

pub async fn worker(sender: &mpsc::Sender<Update>) -> anyhow::Result<()> {
    let mut interval = interval(Duration::from_mins(1));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut last_tick: i64 = 0;

    loop {
        interval.tick().await;

        let tick = Utc::now().timestamp() / TimeDelta::minutes(30).num_seconds();
        if tick == last_tick {
            continue;
        }

        match inquire().await {
            Ok(data) => {
                info!("OpenWeather updated");

                last_tick = tick;
                sender.send(Update::OpenWeather(data)).await?;
            }
            Err(e) => {
                error!("Failed to update OpenWeather: {e:?}");
            }
        }
    }
}

async fn inquire() -> anyhow::Result<OpenWeatherData> {
    const BASE_URL: &str = "https://api.openweathermap.org/data/2.5/weather";

    let settings::OpenWeather { lat, lon, api_key } = settings::openweather();

    let params: [(&str, &str); 3] = [
        ("lat", &lat.to_compact_string()),
        ("lon", &lon.to_compact_string()),
        ("appid", api_key),
    ];
    let url = Url::parse_with_params(BASE_URL, params)?;

    let msg: Message = REST_CLIENT.get(url).send().await?.json().await?;
    let Some(weahter) = msg.weather.into_iter().next() else {
        bail!("Bad message format");
    };
    let icon = block_in_place(|| icon(&weahter.icon))?;

    Ok(OpenWeatherData {
        icon,
        description: weahter.description,
        pressure: msg.main.pressure,
    })
}

fn icon(code: &str) -> anyhow::Result<RgbaImage> {
    let data: &[u8] = match code {
        "01d" => include_bytes!("../icons/01d_t@4x.png"),
        "01n" => include_bytes!("../icons/01n_t@4x.png"),
        "02d" => include_bytes!("../icons/02d_t@4x.png"),
        "02n" => include_bytes!("../icons/02n_t@4x.png"),
        "03d" => include_bytes!("../icons/03d_t@4x.png"),
        "03n" => include_bytes!("../icons/03n_t@4x.png"),
        "04d" => include_bytes!("../icons/04d_t@4x.png"),
        "04n" => include_bytes!("../icons/04n_t@4x.png"),
        "09d" => include_bytes!("../icons/09d_t@4x.png"),
        "09n" => include_bytes!("../icons/09n_t@4x.png"),
        "10d" => include_bytes!("../icons/10d_t@4x.png"),
        "10n" => include_bytes!("../icons/10n_t@4x.png"),
        "11d" => include_bytes!("../icons/11d_t@4x.png"),
        "11n" => include_bytes!("../icons/11n_t@4x.png"),
        "13d" => include_bytes!("../icons/13d_t@4x.png"),
        "13n" => include_bytes!("../icons/13n_t@4x.png"),
        "50d" => include_bytes!("../icons/50d_t@4x.png"),
        "50n" => include_bytes!("../icons/50n_t@4x.png"),
        _ => bail!("Invalid icon code"),
    };
    let image = image::load_from_memory(data)?;
    let image = image.into_bgra8();

    Ok(image)
}
