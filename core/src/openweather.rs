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
use crate::image::AlignedImage;
use crate::settings;

#[derive(Debug, Clone, Copy)]
enum Icons {
    DayCloudyGusts,
    DayFog,
    DayHail,
    DayHaze,
    DayLightning,
    DayRainMix,
    DayRain,
    DayShowers,
    DaySleet,
    DaySnow,
    DaySprinkle,
    DayStormShowers,
    DaySunnyOvercast,
    DaySunny,
    DayThunderstorm,
    Dust,
    Hot,
    Hurricane,
    NightAltCloudyGusts,
    NightAltCloudy,
    NightAltHail,
    NightAltLightning,
    NightAltRainMix,
    NightAltRain,
    NightAltShowers,
    NightAltSleet,
    NightAltSnow,
    NightAltSprinkle,
    NightAltStormShowers,
    NightAltThunderstorm,
    NightClear,
    NightFog,
    Smoke,
    SnowflakeCold,
    StrongWind,
    Tornado,
}

impl Icons {
    #[allow(clippy::too_many_lines)]
    fn from_openweather_id(id: i32, is_day: bool) -> anyhow::Result<Self> {
        #[allow(clippy::match_same_arms)]
        let icon = match (id, is_day) {
            (200, true) => Icons::DayThunderstorm,
            (201, true) => Icons::DayThunderstorm,
            (202, true) => Icons::DayThunderstorm,
            (210, true) => Icons::DayLightning,
            (211, true) => Icons::DayLightning,
            (212, true) => Icons::DayLightning,
            (221, true) => Icons::DayLightning,
            (230, true) => Icons::DayThunderstorm,
            (231, true) => Icons::DayThunderstorm,
            (232, true) => Icons::DayThunderstorm,
            (300, true) => Icons::DaySprinkle,
            (301, true) => Icons::DaySprinkle,
            (302, true) => Icons::DayRain,
            (310, true) => Icons::DayRain,
            (311, true) => Icons::DayRain,
            (312, true) => Icons::DayRain,
            (313, true) => Icons::DayRain,
            (314, true) => Icons::DayRain,
            (321, true) => Icons::DaySprinkle,
            (500, true) => Icons::DaySprinkle,
            (501, true) => Icons::DayRain,
            (502, true) => Icons::DayRain,
            (503, true) => Icons::DayRain,
            (504, true) => Icons::DayRain,
            (511, true) => Icons::DayRainMix,
            (520, true) => Icons::DayShowers,
            (521, true) => Icons::DayShowers,
            (522, true) => Icons::DayShowers,
            (531, true) => Icons::DayStormShowers,
            (600, true) => Icons::DaySnow,
            (601, true) => Icons::DaySleet,
            (602, true) => Icons::DaySnow,
            (611, true) => Icons::DayRainMix,
            (612, true) => Icons::DayRainMix,
            (615, true) => Icons::DayRainMix,
            (616, true) => Icons::DayRainMix,
            (620, true) => Icons::DayRainMix,
            (621, true) => Icons::DaySnow,
            (622, true) => Icons::DaySnow,
            (701, true) => Icons::DayShowers,
            (711, true) => Icons::Smoke,
            (721, true) => Icons::DayHaze,
            (731, true) => Icons::Dust,
            (741, true) => Icons::DayFog,
            (761, true) => Icons::Dust,
            (762, true) => Icons::Dust,
            (781, true) => Icons::Tornado,
            (800, true) => Icons::DaySunny,
            (801, true) => Icons::DayCloudyGusts,
            (802, true) => Icons::DayCloudyGusts,
            (803, true) => Icons::DayCloudyGusts,
            (804, true) => Icons::DaySunnyOvercast,
            (900, true) => Icons::Tornado,
            (902, true) => Icons::Hurricane,
            (903, true) => Icons::SnowflakeCold,
            (904, true) => Icons::Hot,
            (906, true) => Icons::DayHail,
            (957, true) => Icons::StrongWind,
            (200, false) => Icons::NightAltThunderstorm,
            (201, false) => Icons::NightAltThunderstorm,
            (202, false) => Icons::NightAltThunderstorm,
            (210, false) => Icons::NightAltLightning,
            (211, false) => Icons::NightAltLightning,
            (212, false) => Icons::NightAltLightning,
            (221, false) => Icons::NightAltLightning,
            (230, false) => Icons::NightAltThunderstorm,
            (231, false) => Icons::NightAltThunderstorm,
            (232, false) => Icons::NightAltThunderstorm,
            (300, false) => Icons::NightAltSprinkle,
            (301, false) => Icons::NightAltSprinkle,
            (302, false) => Icons::NightAltRain,
            (310, false) => Icons::NightAltRain,
            (311, false) => Icons::NightAltRain,
            (312, false) => Icons::NightAltRain,
            (313, false) => Icons::NightAltRain,
            (314, false) => Icons::NightAltRain,
            (321, false) => Icons::NightAltSprinkle,
            (500, false) => Icons::NightAltSprinkle,
            (501, false) => Icons::NightAltRain,
            (502, false) => Icons::NightAltRain,
            (503, false) => Icons::NightAltRain,
            (504, false) => Icons::NightAltRain,
            (511, false) => Icons::NightAltRainMix,
            (520, false) => Icons::NightAltShowers,
            (521, false) => Icons::NightAltShowers,
            (522, false) => Icons::NightAltShowers,
            (531, false) => Icons::NightAltStormShowers,
            (600, false) => Icons::NightAltSnow,
            (601, false) => Icons::NightAltSleet,
            (602, false) => Icons::NightAltSnow,
            (611, false) => Icons::NightAltRainMix,
            (612, false) => Icons::NightAltRainMix,
            (615, false) => Icons::NightAltRainMix,
            (616, false) => Icons::NightAltRainMix,
            (620, false) => Icons::NightAltRainMix,
            (621, false) => Icons::NightAltSnow,
            (622, false) => Icons::NightAltSnow,
            (701, false) => Icons::NightAltShowers,
            (711, false) => Icons::Smoke,
            (721, false) => Icons::DayHaze,
            (731, false) => Icons::Dust,
            (741, false) => Icons::NightFog,
            (761, false) => Icons::Dust,
            (762, false) => Icons::Dust,
            (781, false) => Icons::Tornado,
            (800, false) => Icons::NightClear,
            (801, false) => Icons::NightAltCloudyGusts,
            (802, false) => Icons::NightAltCloudyGusts,
            (803, false) => Icons::NightAltCloudyGusts,
            (804, false) => Icons::NightAltCloudy,
            (900, false) => Icons::Tornado,
            (902, false) => Icons::Hurricane,
            (903, false) => Icons::SnowflakeCold,
            (904, false) => Icons::Hot,
            (906, false) => Icons::NightAltHail,
            (957, false) => Icons::StrongWind,
            _ => bail!("Bad ID"),
        };
        Ok(icon)
    }

    fn as_bytes(self) -> &'static [u8] {
        match self {
            Icons::DayCloudyGusts => include_bytes!("../icons/wi-day-cloudy-gusts.webp"),
            Icons::DayFog => include_bytes!("../icons/wi-day-fog.webp"),
            Icons::DayHail => include_bytes!("../icons/wi-day-hail.webp"),
            Icons::DayHaze => include_bytes!("../icons/wi-day-haze.webp"),
            Icons::DayLightning => include_bytes!("../icons/wi-day-lightning.webp"),
            Icons::DayRainMix => include_bytes!("../icons/wi-day-rain-mix.webp"),
            Icons::DayRain => include_bytes!("../icons/wi-day-rain.webp"),
            Icons::DayShowers => include_bytes!("../icons/wi-day-showers.webp"),
            Icons::DaySleet => include_bytes!("../icons/wi-day-sleet.webp"),
            Icons::DaySnow => include_bytes!("../icons/wi-day-snow.webp"),
            Icons::DaySprinkle => include_bytes!("../icons/wi-day-sprinkle.webp"),
            Icons::DayStormShowers => include_bytes!("../icons/wi-day-storm-showers.webp"),
            Icons::DaySunnyOvercast => include_bytes!("../icons/wi-day-sunny-overcast.webp"),
            Icons::DaySunny => include_bytes!("../icons/wi-day-sunny.webp"),
            Icons::DayThunderstorm => include_bytes!("../icons/wi-day-thunderstorm.webp"),
            Icons::Dust => include_bytes!("../icons/wi-dust.webp"),
            Icons::Hot => include_bytes!("../icons/wi-hot.webp"),
            Icons::Hurricane => include_bytes!("../icons/wi-hurricane.webp"),
            Icons::NightAltCloudyGusts => include_bytes!("../icons/wi-night-alt-cloudy-gusts.webp"),
            Icons::NightAltCloudy => include_bytes!("../icons/wi-night-alt-cloudy.webp"),
            Icons::NightAltHail => include_bytes!("../icons/wi-night-alt-hail.webp"),
            Icons::NightAltLightning => include_bytes!("../icons/wi-night-alt-lightning.webp"),
            Icons::NightAltRainMix => include_bytes!("../icons/wi-night-alt-rain-mix.webp"),
            Icons::NightAltRain => include_bytes!("../icons/wi-night-alt-rain.webp"),
            Icons::NightAltShowers => include_bytes!("../icons/wi-night-alt-showers.webp"),
            Icons::NightAltSleet => include_bytes!("../icons/wi-night-alt-sleet.webp"),
            Icons::NightAltSnow => include_bytes!("../icons/wi-night-alt-snow.webp"),
            Icons::NightAltSprinkle => include_bytes!("../icons/wi-night-alt-sprinkle.webp"),
            Icons::NightAltStormShowers => include_bytes!("../icons/wi-night-alt-storm-showers.webp"),
            Icons::NightAltThunderstorm => include_bytes!("../icons/wi-night-alt-thunderstorm.webp"),
            Icons::NightClear => include_bytes!("../icons/wi-night-clear.webp"),
            Icons::NightFog => include_bytes!("../icons/wi-night-fog.webp"),
            Icons::Smoke => include_bytes!("../icons/wi-smoke.webp"),
            Icons::SnowflakeCold => include_bytes!("../icons/wi-snowflake-cold.webp"),
            Icons::StrongWind => include_bytes!("../icons/wi-strong-wind.webp"),
            Icons::Tornado => include_bytes!("../icons/wi-tornado.webp"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Message {
    weather: Vec<Weather>,
    main: Main,
    dt: i64,
    sys: Sys,
}

#[derive(Debug, Deserialize)]
struct Weather {
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
    pub icon: AlignedImage,
    pub description: CompactString,
    pub pressure: i32,
}

pub async fn worker(sender: &mpsc::Sender<Update>) -> anyhow::Result<()> {
    let mut interval = interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut last_tick: i64 = 0;

    loop {
        interval.tick().await;

        let tick = (Utc::now().timestamp() - 20) / TimeDelta::minutes(10).num_seconds();
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

    let params: [(&str, &str); _] = [
        ("lat", &lat.to_compact_string()),
        ("lon", &lon.to_compact_string()),
        ("appid", api_key),
    ];
    let url = Url::parse_with_params(BASE_URL, params)?;

    let msg: Message = REST_CLIENT.get(url).send().await?.json().await?;
    let Some(weather) = msg.weather.into_iter().next() else {
        bail!("Bad message format (weather is empty)");
    };
    let icon = block_in_place(|| {
        let is_day = (msg.sys.sunrise..msg.sys.sunset).contains(&msg.dt);
        let icon = Icons::from_openweather_id(weather.id, is_day)?;
        let image = image::load_from_memory(icon.as_bytes())?;

        anyhow::Ok(image.into())
    })?;

    Ok(OpenWeatherData {
        icon,
        description: weather.description,
        pressure: msg.main.pressure,
    })
}
