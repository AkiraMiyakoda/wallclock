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
use crate::alphablend::AlignedRgbaImage;
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
    fn from_openweather_id(id: i32, is_day: bool) -> anyhow::Result<Self> {
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
            Icons::DayCloudyGusts => include_bytes!("../icons/wi-day-cloudy-gusts.png"),
            Icons::DayFog => include_bytes!("../icons/wi-day-fog.png"),
            Icons::DayHail => include_bytes!("../icons/wi-day-hail.png"),
            Icons::DayHaze => include_bytes!("../icons/wi-day-haze.png"),
            Icons::DayLightning => include_bytes!("../icons/wi-day-lightning.png"),
            Icons::DayRainMix => include_bytes!("../icons/wi-day-rain-mix.png"),
            Icons::DayRain => include_bytes!("../icons/wi-day-rain.png"),
            Icons::DayShowers => include_bytes!("../icons/wi-day-showers.png"),
            Icons::DaySleet => include_bytes!("../icons/wi-day-sleet.png"),
            Icons::DaySnow => include_bytes!("../icons/wi-day-snow.png"),
            Icons::DaySprinkle => include_bytes!("../icons/wi-day-sprinkle.png"),
            Icons::DayStormShowers => include_bytes!("../icons/wi-day-storm-showers.png"),
            Icons::DaySunnyOvercast => include_bytes!("../icons/wi-day-sunny-overcast.png"),
            Icons::DaySunny => include_bytes!("../icons/wi-day-sunny.png"),
            Icons::DayThunderstorm => include_bytes!("../icons/wi-day-thunderstorm.png"),
            Icons::Dust => include_bytes!("../icons/wi-dust.png"),
            Icons::Hot => include_bytes!("../icons/wi-hot.png"),
            Icons::Hurricane => include_bytes!("../icons/wi-hurricane.png"),
            Icons::NightAltCloudyGusts => include_bytes!("../icons/wi-night-alt-cloudy-gusts.png"),
            Icons::NightAltCloudy => include_bytes!("../icons/wi-night-alt-cloudy.png"),
            Icons::NightAltHail => include_bytes!("../icons/wi-night-alt-hail.png"),
            Icons::NightAltLightning => include_bytes!("../icons/wi-night-alt-lightning.png"),
            Icons::NightAltRainMix => include_bytes!("../icons/wi-night-alt-rain-mix.png"),
            Icons::NightAltRain => include_bytes!("../icons/wi-night-alt-rain.png"),
            Icons::NightAltShowers => include_bytes!("../icons/wi-night-alt-showers.png"),
            Icons::NightAltSleet => include_bytes!("../icons/wi-night-alt-sleet.png"),
            Icons::NightAltSnow => include_bytes!("../icons/wi-night-alt-snow.png"),
            Icons::NightAltSprinkle => include_bytes!("../icons/wi-night-alt-sprinkle.png"),
            Icons::NightAltStormShowers => include_bytes!("../icons/wi-night-alt-storm-showers.png"),
            Icons::NightAltThunderstorm => include_bytes!("../icons/wi-night-alt-thunderstorm.png"),
            Icons::NightClear => include_bytes!("../icons/wi-night-clear.png"),
            Icons::NightFog => include_bytes!("../icons/wi-night-fog.png"),
            Icons::Smoke => include_bytes!("../icons/wi-smoke.png"),
            Icons::SnowflakeCold => include_bytes!("../icons/wi-snowflake-cold.png"),
            Icons::StrongWind => include_bytes!("../icons/wi-strong-wind.png"),
            Icons::Tornado => include_bytes!("../icons/wi-tornado.png"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Message {
    data: Vec<Data>,
}

#[derive(Debug, Deserialize)]
struct Data {
    dt: i64,
    sunrise: i64,
    sunset: i64,
    pressure: i32,
    weather: Vec<Weather>,
    rain: Option<RainOrSnow>,
    snow: Option<RainOrSnow>,
}

#[derive(Debug, Deserialize)]
struct Weather {
    id: i32,
    description: CompactString,
}

#[derive(Debug, Deserialize)]
struct RainOrSnow {
    #[serde(rename = "1h")]
    hourly: f32,
}

#[derive(Debug)]
pub struct OpenWeatherData {
    pub icon: AlignedRgbaImage,
    pub description: CompactString,
    pub pressure: i32,
    #[allow(dead_code)]
    pub rainfall: f32,
}

pub async fn worker(sender: &mpsc::Sender<Update>) -> anyhow::Result<()> {
    let mut interval = interval(Duration::from_secs(5));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut last_tick: i64 = 0;

    loop {
        interval.tick().await;

        let tick = (Utc::now().timestamp() - 10) / TimeDelta::minutes(10).num_seconds();
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
    const BASE_URL: &str = "https://api.openweathermap.org/data/4.0/onecall/current";

    let settings::OpenWeather { lat, lon, api_key } = settings::openweather();

    let params: [(&str, &str); _] = [
        ("lat", &lat.to_compact_string()),
        ("lon", &lon.to_compact_string()),
        ("appid", api_key),
        ("units", "metric"),
        ("lang", "en"),
    ];
    let url = Url::parse_with_params(BASE_URL, params)?;

    let msg: Message = REST_CLIENT.get(url).send().await?.json().await?;
    let Some(data) = msg.data.into_iter().next() else {
        bail!("Bad message format (data is empty)");
    };
    let Some(weather) = data.weather.into_iter().next() else {
        bail!("Bad message format (weather is empty)");
    };
    let icon = block_in_place(|| {
        let is_day = (data.sunrise..data.sunset).contains(&data.dt);
        let icon = Icons::from_openweather_id(weather.id, is_day)?;
        let image = image::load_from_memory(icon.as_bytes())?;

        anyhow::Ok(image.into())
    })?;
    let rainfall = data.rain.or(data.snow).map(|rain| rain.hourly).unwrap_or(0.0);

    Ok(OpenWeatherData {
        icon,
        description: weather.description,
        pressure: data.pressure,
        rainfall,
    })
}
