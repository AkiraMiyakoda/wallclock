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

#[derive(Debug, Clone, Copy)]
struct Dimensions {
    width: u16,
    height: u16,
}

impl From<Dimensions> for (u32, u32) {
    fn from(val: Dimensions) -> Self {
        (val.width.into(), val.height.into())
    }
}

impl From<Dimensions> for (i32, i32) {
    fn from(val: Dimensions) -> Self {
        (val.width.into(), val.height.into())
    }
}

impl From<Dimensions> for (u16, u16) {
    fn from(val: Dimensions) -> Self {
        (val.width, val.height)
    }
}

const SCREEN_DIMENSIONS: Dimensions = Dimensions {
    width: 2560,
    height: 1600,
};
const CHANNEL_SIZE: usize = 8;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

static REST_CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

#[tokio::main(worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    logger::init();

    let (sender, receiver) = mpsc::channel(CHANNEL_SIZE);

    // Exit the process when any worker exits, regardless of whether it succeeded or failed.
    select! {
        result = switchbot::worker(sender.clone()) => result,
        result = openweather::worker(sender.clone()) => result,
        result = wallpaper::worker(sender) => result,
        result = display::worker(receiver) => result,
    }
}
