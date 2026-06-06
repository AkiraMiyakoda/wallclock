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
use crate::settings;

#[derive(Debug, Deserialize)]
struct Message {
    weather: Vec<WeatherRow>,
    main: Main,
    dt: i64,
    sys: Sys,
}

#[derive(Debug, Deserialize)]
struct WeatherRow {
    id: i32,
    description: CompactString,
}

#[derive(Debug, Deserialize)]
struct Main {
    pressure: i32,
}

#[derive(Debug, Deserialize)]
struct Sys {
    sunrise: i64,
    sunset: i64,
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
    let is_day = (msg.sys.sunrise..msg.sys.sunset).contains(&msg.dt);
    let icon = block_in_place(|| id_to_icon(weahter.id, is_day))?;

    Ok(OpenWeatherData {
        icon,
        description: weahter.description,
        pressure: msg.main.pressure,
    })
}

fn id_to_icon(id: i32, is_day: bool) -> anyhow::Result<RgbaImage> {
    let name = match (id, is_day) {
        (200, true) => "day-thunderstorm",
        (201, true) => "day-thunderstorm",
        (202, true) => "day-thunderstorm",
        (210, true) => "day-lightning",
        (211, true) => "day-lightning",
        (212, true) => "day-lightning",
        (221, true) => "day-lightning",
        (230, true) => "day-thunderstorm",
        (231, true) => "day-thunderstorm",
        (232, true) => "day-thunderstorm",
        (300, true) => "day-sprinkle",
        (301, true) => "day-sprinkle",
        (302, true) => "day-rain",
        (310, true) => "day-rain",
        (311, true) => "day-rain",
        (312, true) => "day-rain",
        (313, true) => "day-rain",
        (314, true) => "day-rain",
        (321, true) => "day-sprinkle",
        (500, true) => "day-sprinkle",
        (501, true) => "day-rain",
        (502, true) => "day-rain",
        (503, true) => "day-rain",
        (504, true) => "day-rain",
        (511, true) => "day-rain-mix",
        (520, true) => "day-showers",
        (521, true) => "day-showers",
        (522, true) => "day-showers",
        (531, true) => "day-storm-showers",
        (600, true) => "day-snow",
        (601, true) => "day-sleet",
        (602, true) => "day-snow",
        (611, true) => "day-rain-mix",
        (612, true) => "day-rain-mix",
        (615, true) => "day-rain-mix",
        (616, true) => "day-rain-mix",
        (620, true) => "day-rain-mix",
        (621, true) => "day-snow",
        (622, true) => "day-snow",
        (701, true) => "day-showers",
        (711, true) => "smoke",
        (721, true) => "day-haze",
        (731, true) => "dust",
        (741, true) => "day-fog",
        (761, true) => "dust",
        (762, true) => "dust",
        (781, true) => "tornado",
        (800, true) => "day-sunny",
        (801, true) => "day-cloudy-gusts",
        (802, true) => "day-cloudy-gusts",
        (803, true) => "day-cloudy-gusts",
        (804, true) => "day-sunny-overcast",
        (900, true) => "tornado",
        (902, true) => "hurricane",
        (903, true) => "snowflake-cold",
        (904, true) => "hot",
        (906, true) => "day-hail",
        (957, true) => "strong-wind",
        (200, false) => "night-alt-thunderstorm",
        (201, false) => "night-alt-thunderstorm",
        (202, false) => "night-alt-thunderstorm",
        (210, false) => "night-alt-lightning",
        (211, false) => "night-alt-lightning",
        (212, false) => "night-alt-lightning",
        (221, false) => "night-alt-lightning",
        (230, false) => "night-alt-thunderstorm",
        (231, false) => "night-alt-thunderstorm",
        (232, false) => "night-alt-thunderstorm",
        (300, false) => "night-alt-sprinkle",
        (301, false) => "night-alt-sprinkle",
        (302, false) => "night-alt-rain",
        (310, false) => "night-alt-rain",
        (311, false) => "night-alt-rain",
        (312, false) => "night-alt-rain",
        (313, false) => "night-alt-rain",
        (314, false) => "night-alt-rain",
        (321, false) => "night-alt-sprinkle",
        (500, false) => "night-alt-sprinkle",
        (501, false) => "night-alt-rain",
        (502, false) => "night-alt-rain",
        (503, false) => "night-alt-rain",
        (504, false) => "night-alt-rain",
        (511, false) => "night-alt-rain-mix",
        (520, false) => "night-alt-showers",
        (521, false) => "night-alt-showers",
        (522, false) => "night-alt-showers",
        (531, false) => "night-alt-storm-showers",
        (600, false) => "night-alt-snow",
        (601, false) => "night-alt-sleet",
        (602, false) => "night-alt-snow",
        (611, false) => "night-alt-rain-mix",
        (612, false) => "night-alt-rain-mix",
        (615, false) => "night-alt-rain-mix",
        (616, false) => "night-alt-rain-mix",
        (620, false) => "night-alt-rain-mix",
        (621, false) => "night-alt-snow",
        (622, false) => "night-alt-snow",
        (701, false) => "night-alt-showers",
        (711, false) => "smoke",
        (721, false) => "day-haze",
        (731, false) => "dust",
        (741, false) => "night-fog",
        (761, false) => "dust",
        (762, false) => "dust",
        (781, false) => "tornado",
        (800, false) => "night-clear",
        (801, false) => "night-alt-cloudy-gusts",
        (802, false) => "night-alt-cloudy-gusts",
        (803, false) => "night-alt-cloudy-gusts",
        (804, false) => "night-alt-cloudy",
        (900, false) => "tornado",
        (902, false) => "hurricane",
        (903, false) => "snowflake-cold",
        (904, false) => "hot",
        (906, false) => "night-alt-hail",
        (957, false) => "strong-wind",
        _ => bail!("Bad icon ID"),
    };
    let data: &[u8] = match name {
        "day-cloudy-gusts" => include_bytes!("../icons/wi-day-cloudy-gusts.png"),
        "day-fog" => include_bytes!("../icons/wi-day-fog.png"),
        "day-hail" => include_bytes!("../icons/wi-day-hail.png"),
        "day-haze" => include_bytes!("../icons/wi-day-haze.png"),
        "day-lightning" => include_bytes!("../icons/wi-day-lightning.png"),
        "day-rain" => include_bytes!("../icons/wi-day-rain.png"),
        "day-rain-mix" => include_bytes!("../icons/wi-day-rain-mix.png"),
        "day-showers" => include_bytes!("../icons/wi-day-showers.png"),
        "day-sleet" => include_bytes!("../icons/wi-day-sleet.png"),
        "day-snow" => include_bytes!("../icons/wi-day-snow.png"),
        "day-sprinkle" => include_bytes!("../icons/wi-day-sprinkle.png"),
        "day-storm-showers" => include_bytes!("../icons/wi-day-storm-showers.png"),
        "day-sunny" => include_bytes!("../icons/wi-day-sunny.png"),
        "day-sunny-overcast" => include_bytes!("../icons/wi-day-sunny-overcast.png"),
        "day-thunderstorm" => include_bytes!("../icons/wi-day-thunderstorm.png"),
        "dust" => include_bytes!("../icons/wi-dust.png"),
        "hot" => include_bytes!("../icons/wi-hot.png"),
        "hurricane" => include_bytes!("../icons/wi-hurricane.png"),
        "night-alt-cloudy" => include_bytes!("../icons/wi-night-alt-cloudy.png"),
        "night-alt-cloudy-gusts" => include_bytes!("../icons/wi-night-alt-cloudy-gusts.png"),
        "night-alt-hail" => include_bytes!("../icons/wi-night-alt-hail.png"),
        "night-alt-lightning" => include_bytes!("../icons/wi-night-alt-lightning.png"),
        "night-alt-rain" => include_bytes!("../icons/wi-night-alt-rain.png"),
        "night-alt-rain-mix" => include_bytes!("../icons/wi-night-alt-rain-mix.png"),
        "night-alt-showers" => include_bytes!("../icons/wi-night-alt-showers.png"),
        "night-alt-sleet" => include_bytes!("../icons/wi-night-alt-sleet.png"),
        "night-alt-snow" => include_bytes!("../icons/wi-night-alt-snow.png"),
        "night-alt-sprinkle" => include_bytes!("../icons/wi-night-alt-sprinkle.png"),
        "night-alt-storm-showers" => include_bytes!("../icons/wi-night-alt-storm-showers.png"),
        "night-alt-thunderstorm" => include_bytes!("../icons/wi-night-alt-thunderstorm.png"),
        "night-clear" => include_bytes!("../icons/wi-night-clear.png"),
        "night-fog" => include_bytes!("../icons/wi-night-fog.png"),
        "smoke" => include_bytes!("../icons/wi-smoke.png"),
        "snowflake-cold" => include_bytes!("../icons/wi-snowflake-cold.png"),
        "strong-wind" => include_bytes!("../icons/wi-strong-wind.png"),
        "tornado" => include_bytes!("../icons/wi-tornado.png"),
        _ => panic!("Bad icon name"),
    };
    let image = image::load_from_memory(data)?;

    let (width, height) = (image.width(), image.height());
    let raw = image.into_rgba8().into_raw_bgra();
    let image = RgbaImage::from_raw(width, height, raw).unwrap();

    Ok(image)
}
