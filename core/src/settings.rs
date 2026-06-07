// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::borrow::Cow;
use std::env;
use std::fs;
use std::sync::LazyLock;

use compact_str::CompactString;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Settings {
    switchbot: Switchbot,
    openweather: OpenWeather,
    wallhaven: Wallhaven,
    drm: Drm,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Switchbot {
    pub token: String,
    pub secret: String,
    pub devices: (SwitchBotDevice, SwitchBotDevice, SwitchBotDevice),
}

#[derive(Debug, Clone, Deserialize)]
pub struct SwitchBotDevice {
    pub id: CompactString,
    pub name: CompactString,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OpenWeather {
    pub lat: f32,
    pub lon: f32,
    pub api_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Wallhaven {
    pub query: CompactString,
    pub categories: CompactString,
    pub purity: CompactString,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Drm {
    pub device: String,
}

impl Settings {
    pub fn load() -> anyhow::Result<Self> {
        const PATH_ENV: &str = "CONFIG_PATH";
        const DEFAULT_PATH: &str = "/run/secrets/settings.toml";

        let path = env::var(PATH_ENV).map_or(DEFAULT_PATH.into(), Cow::from);
        let toml = fs::read_to_string(path.as_ref())?;
        let settings: Settings = toml::from_str(&toml)?;
        Ok(settings)
    }
}

static INSTANCE: LazyLock<Settings> = LazyLock::new(|| Settings::load().expect("Failed to load settings"));

pub fn switchbot() -> &'static Switchbot {
    &INSTANCE.switchbot
}

pub fn openweather() -> &'static OpenWeather {
    &INSTANCE.openweather
}

pub fn wallhaven() -> &'static Wallhaven {
    &INSTANCE.wallhaven
}

pub fn drm() -> &'static Drm {
    &INSTANCE.drm
}
