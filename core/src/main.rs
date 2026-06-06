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
mod openweather;
mod settings;
mod switchbot;
mod tick;
mod wallpaper;

#[derive(Debug)]
enum Update {
    Tick,
    SwitchBot(SwitchBotData),
    OpenWeather(OpenWeatherData),
    Wallpaper(WallpaperData),
}

struct Dimensions(u16, u16);

impl Into<(u32, u32)> for Dimensions {
    fn into(self) -> (u32, u32) {
        (self.0.into(), self.1.into())
    }
}

impl Into<(i32, i32)> for Dimensions {
    fn into(self) -> (i32, i32) {
        (self.0.into(), self.1.into())
    }
}

impl Into<(u16, u16)> for Dimensions {
    fn into(self) -> (u16, u16) {
        (self.0, self.1)
    }
}

const SCREEN_DIMENSIONS: Dimensions = Dimensions(2560, 1600);
const CHANNEL_SIZE: usize = 16;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

static REST_CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

#[tokio::main(worker_threads = 1)]
async fn main() -> anyhow::Result<()> {
    logger::init();

    let (sender, receiver) = mpsc::channel(CHANNEL_SIZE);

    select! {
        result = tick::worker(&sender) => result,
        result = switchbot::worker(&sender) => result,
        result = openweather::worker(&sender) => result,
        result = wallpaper::worker(&sender) => result,
        result = display::worker(receiver) => result,
    }
}
