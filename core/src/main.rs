// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::sync::LazyLock;
use std::time::Duration;

use chrono::Utc;
use mimalloc::MiMalloc;
use reqwest::Client;
use tokio::select;
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use tokio::time::interval;

use crate::switchbot::SwitchBotData;
use crate::wallpaper::WallpaperData;

mod display;
mod settings;
mod switchbot;
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

static WEB_CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

#[tokio::main(worker_threads = 1)]
async fn main() -> anyhow::Result<()> {
    logger::init();

    let (sender, receiver) = mpsc::channel(CHANNEL_SIZE);

    select! {
        result = tick_worker(&sender) => result,
        result = switchbot::worker(&sender) => result,
        result = wallpaper::worker(&sender) => result,
        result = display::worker(receiver) => result,
    }
}

async fn tick_worker(sender: &mpsc::Sender<Update>) -> anyhow::Result<()> {
    let mut interval = interval(Duration::from_millis(100));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut last_tick: i64 = 0;

    loop {
        interval.tick().await;

        let tick = Utc::now().timestamp();
        if tick == last_tick {
            continue;
        }

        last_tick = tick;
        sender.send(Update::Tick).await?;
    }
}
