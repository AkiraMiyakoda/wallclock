// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::env;
use std::fs;
use std::sync::LazyLock;

use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Settings {
    switchbot: SwitchBot,
    openweather: OpenWeather,
    balbird: Balbird,
    drm: Drm,
}

#[derive(Debug, Deserialize)]
pub struct SwitchBot {
    pub token: String,
    pub secret: String,
    pub devices: SwitchBotDevices,
}

#[derive(Debug, Deserialize)]
pub struct SwitchBotDevices {
    pub indoor: SwitchBotDevice,
    pub outdoor: SwitchBotDevice,
    pub tank: SwitchBotDevice,
}

#[derive(Debug, Deserialize)]
pub struct SwitchBotDevice {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
pub struct OpenWeather {
    pub lat: f32,
    pub lon: f32,
    pub api_key: String,
}

#[derive(Debug, Deserialize)]
pub struct Balbird {
    pub api_key: String,
}

#[derive(Debug, Deserialize)]
pub struct Drm {
    pub device: String,
}

impl Settings {
    pub fn load() -> anyhow::Result<Self> {
        const PATH_ENV: &str = "CONFIG_PATH";
        const DEFAULT_PATH: &str = "/run/secrets/settings.toml";

        // CONFIG_PATH can override the default secret-mounted settings file.
        let path = env::var(PATH_ENV).unwrap_or_else(|_| DEFAULT_PATH.to_owned());
        let toml = fs::read_to_string(&path).with_context(|| format!("Failed to read settings file: {path}"))?;
        let settings = toml::from_str(&toml).with_context(|| format!("Failed to parse settings file: {path}"))?;

        Ok(settings)
    }
}

static INSTANCE: LazyLock<Settings> = LazyLock::new(|| Settings::load().expect("Failed to load settings"));

pub fn switchbot() -> &'static SwitchBot {
    &INSTANCE.switchbot
}

pub fn openweather() -> &'static OpenWeather {
    &INSTANCE.openweather
}

pub fn balbird() -> &'static Balbird {
    &INSTANCE.balbird
}

pub fn drm() -> &'static Drm {
    &INSTANCE.drm
}
