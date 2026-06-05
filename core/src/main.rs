// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::sync::LazyLock;

use mimalloc::MiMalloc;
use reqwest::Client;
use tokio::select;
use tokio::sync::mpsc;

use crate::switchbot::SwitchBotData;
use crate::wallpaper::WallpaperData;

mod display;
mod image;
mod settings;
mod switchbot;
mod tick;
mod wallpaper;

#[derive(Debug)]
enum Update {
    Tick,
    SwitchBot(SwitchBotData),
    Wallpaper(WallpaperData),
}

const CHANNEL_SIZE: usize = 16;
const SCREEN_DIMENSIONS: (u32, u32) = (2560, 1600);

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
        result = wallpaper::worker(&sender) => result,
        result = display::worker(receiver) => result,
    }
}
