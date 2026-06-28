// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::bail;
use chrono::TimeDelta;
use chrono::Utc;
use compact_str::CompactString;
use compact_str::ToCompactString;
use log::error;
use log::info;
use reqwest::Url;
use serde::Deserialize;
use tokio::sync::RwLock;
use tokio::task::spawn_blocking;
use tokio::time::MissedTickBehavior;
use tokio::time::interval;

use crate::REST_CLIENT;
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

// Keep these tables sorted by OpenWeather condition ID.
// `from_openweather_id` uses binary search.
const DAY_ICONS: &[(i32, Icons)] = &[
    (200, Icons::DayThunderstorm),
    (201, Icons::DayThunderstorm),
    (202, Icons::DayThunderstorm),
    (210, Icons::DayLightning),
    (211, Icons::DayLightning),
    (212, Icons::DayLightning),
    (221, Icons::DayLightning),
    (230, Icons::DayThunderstorm),
    (231, Icons::DayThunderstorm),
    (232, Icons::DayThunderstorm),
    (300, Icons::DaySprinkle),
    (301, Icons::DaySprinkle),
    (302, Icons::DayRain),
    (310, Icons::DayRain),
    (311, Icons::DayRain),
    (312, Icons::DayRain),
    (313, Icons::DayRain),
    (314, Icons::DayRain),
    (321, Icons::DaySprinkle),
    (500, Icons::DaySprinkle),
    (501, Icons::DayRain),
    (502, Icons::DayRain),
    (503, Icons::DayRain),
    (504, Icons::DayRain),
    (511, Icons::DayRainMix),
    (520, Icons::DayShowers),
    (521, Icons::DayShowers),
    (522, Icons::DayShowers),
    (531, Icons::DayStormShowers),
    (600, Icons::DaySnow),
    (601, Icons::DaySleet),
    (602, Icons::DaySnow),
    (611, Icons::DayRainMix),
    (612, Icons::DayRainMix),
    (615, Icons::DayRainMix),
    (616, Icons::DayRainMix),
    (620, Icons::DayRainMix),
    (621, Icons::DaySnow),
    (622, Icons::DaySnow),
    (701, Icons::DayShowers),
    (711, Icons::Smoke),
    (721, Icons::DayHaze),
    (731, Icons::Dust),
    (741, Icons::DayFog),
    (761, Icons::Dust),
    (762, Icons::Dust),
    (781, Icons::Tornado),
    (800, Icons::DaySunny),
    (801, Icons::DayCloudyGusts),
    (802, Icons::DayCloudyGusts),
    (803, Icons::DayCloudyGusts),
    (804, Icons::DaySunnyOvercast),
    (900, Icons::Tornado),
    (902, Icons::Hurricane),
    (903, Icons::SnowflakeCold),
    (904, Icons::Hot),
    (906, Icons::DayHail),
    (957, Icons::StrongWind),
];

const NIGHT_ICONS: &[(i32, Icons)] = &[
    (200, Icons::NightAltThunderstorm),
    (201, Icons::NightAltThunderstorm),
    (202, Icons::NightAltThunderstorm),
    (210, Icons::NightAltLightning),
    (211, Icons::NightAltLightning),
    (212, Icons::NightAltLightning),
    (221, Icons::NightAltLightning),
    (230, Icons::NightAltThunderstorm),
    (231, Icons::NightAltThunderstorm),
    (232, Icons::NightAltThunderstorm),
    (300, Icons::NightAltSprinkle),
    (301, Icons::NightAltSprinkle),
    (302, Icons::NightAltRain),
    (310, Icons::NightAltRain),
    (311, Icons::NightAltRain),
    (312, Icons::NightAltRain),
    (313, Icons::NightAltRain),
    (314, Icons::NightAltRain),
    (321, Icons::NightAltSprinkle),
    (500, Icons::NightAltSprinkle),
    (501, Icons::NightAltRain),
    (502, Icons::NightAltRain),
    (503, Icons::NightAltRain),
    (504, Icons::NightAltRain),
    (511, Icons::NightAltRainMix),
    (520, Icons::NightAltShowers),
    (521, Icons::NightAltShowers),
    (522, Icons::NightAltShowers),
    (531, Icons::NightAltStormShowers),
    (600, Icons::NightAltSnow),
    (601, Icons::NightAltSleet),
    (602, Icons::NightAltSnow),
    (611, Icons::NightAltRainMix),
    (612, Icons::NightAltRainMix),
    (615, Icons::NightAltRainMix),
    (616, Icons::NightAltRainMix),
    (620, Icons::NightAltRainMix),
    (621, Icons::NightAltSnow),
    (622, Icons::NightAltSnow),
    (701, Icons::NightAltShowers),
    (711, Icons::Smoke),
    // DayHaze is intentional. The icon set has no night haze icon.
    (721, Icons::DayHaze),
    (731, Icons::Dust),
    (741, Icons::NightFog),
    (761, Icons::Dust),
    (762, Icons::Dust),
    (781, Icons::Tornado),
    (800, Icons::NightClear),
    (801, Icons::NightAltCloudyGusts),
    (802, Icons::NightAltCloudyGusts),
    (803, Icons::NightAltCloudyGusts),
    (804, Icons::NightAltCloudy),
    (900, Icons::Tornado),
    (902, Icons::Hurricane),
    (903, Icons::SnowflakeCold),
    (904, Icons::Hot),
    (906, Icons::NightAltHail),
    (957, Icons::StrongWind),
];

impl Icons {
    fn from_openweather_id(id: i32, is_day: bool) -> anyhow::Result<Self> {
        let table = if is_day { DAY_ICONS } else { NIGHT_ICONS };

        table
            .binary_search_by_key(&id, |&(weather_id, _)| weather_id)
            .map(|index| table[index].1)
            .map_err(|_| anyhow!("Bad OpenWeather condition ID: {id}, is_day: {is_day}"))
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
struct WeatherResponse {
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

static LATEST_DATA: LazyLock<RwLock<Option<Arc<OpenWeatherData>>>> = LazyLock::new(|| RwLock::new(None));

pub async fn get_latest() -> Option<Arc<OpenWeatherData>> {
    LATEST_DATA.read().await.clone()
}

pub async fn worker() -> anyhow::Result<()> {
    let mut interval = interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut last_tick: i64 = -1;

    loop {
        interval.tick().await;

        // Run about 20 seconds after each 10-minute boundary,
        // so this worker does not run at the same time as the others.
        let tick = (Utc::now().timestamp() - 20) / TimeDelta::minutes(10).num_seconds();
        if tick == last_tick {
            continue;
        }

        match inquire().await {
            Ok(data) => {
                let data = Arc::new(data);
                *LATEST_DATA.write().await = Some(data);

                info!("OpenWeather updated");
            }
            Err(e) => {
                error!("Failed to update OpenWeather: {e:?}");
            }
        }

        // Update this even on failure to avoid retrying every second.
        last_tick = tick;
    }
}

async fn inquire() -> anyhow::Result<OpenWeatherData> {
    const BASE_URL: &str = "https://api.openweathermap.org/data/2.5/weather";

    let settings::OpenWeather { lat, lon, api_key } = settings::openweather();

    let lat = lat.to_compact_string();
    let lon = lon.to_compact_string();
    let params = [
        ("lat", lat.as_str()),
        ("lon", lon.as_str()),
        ("appid", api_key.as_str()),
        ("lang", "en"),
    ];
    let url = Url::parse_with_params(BASE_URL, params)?;

    let response: WeatherResponse = REST_CLIENT
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let Some(weather) = response.weather.into_iter().next() else {
        bail!("OpenWeather API returned an empty weather list");
    };

    let is_day = (response.sys.sunrise..response.sys.sunset).contains(&response.dt);
    let weather_id = weather.id;

    let icon = spawn_blocking(move || {
        let icon = Icons::from_openweather_id(weather_id, is_day)?;
        let image = image::load_from_memory(icon.as_bytes())?;

        anyhow::Ok(image.into())
    })
    .await??;

    Ok(OpenWeatherData {
        icon,
        description: weather.description,
        pressure: response.main.pressure,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_tables_are_sorted() {
        for table in [DAY_ICONS, NIGHT_ICONS] {
            assert!(table.windows(2).all(|w| w[0].0 < w[1].0));
        }
    }

    #[test]
    fn maps_known_weather_ids() {
        assert!(matches!(
            Icons::from_openweather_id(800, true).unwrap(),
            Icons::DaySunny
        ));
        assert!(matches!(
            Icons::from_openweather_id(800, false).unwrap(),
            Icons::NightClear
        ));
    }
}
