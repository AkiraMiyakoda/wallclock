// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::sync::LazyLock;

use mimalloc::MiMalloc;
use reqwest::Client;
use tokio::select;
use tokio::sync::mpsc;

use crate::openweather::OpenWeatherData;
use crate::switchbot::SwitchBotData;
use crate::wallpaper::WallpaperData;

mod display;
mod image;
mod openweather;
mod settings;
mod switchbot;
mod wallpaper;

#[derive(Debug)]
enum Update {
    SwitchBot(SwitchBotData),
    OpenWeather(OpenWeatherData),
    Wallpaper(WallpaperData),
}

struct Dimensions(u16, u16);

impl From<Dimensions> for (u32, u32) {
    fn from(val: Dimensions) -> Self {
        (val.0.into(), val.1.into())
    }
}

impl From<Dimensions> for (i32, i32) {
    fn from(val: Dimensions) -> Self {
        (val.0.into(), val.1.into())
    }
}

impl From<Dimensions> for (u16, u16) {
    fn from(val: Dimensions) -> Self {
        (val.0, val.1)
    }
}

const SCREEN_DIMENSIONS: Dimensions = Dimensions(2560, 1600);
const CHANNEL_SIZE: usize = 8;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

static REST_CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

#[tokio::main(worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    logger::init();

    let (sender, receiver) = mpsc::channel(CHANNEL_SIZE);

    select! {
        result = switchbot::worker(&sender) => result,
        result = openweather::worker(&sender) => result,
        result = wallpaper::worker(&sender) => result,
        result = display::worker(receiver) => result,
    }
}
