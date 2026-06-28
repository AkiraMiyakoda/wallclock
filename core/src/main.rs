// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::sync::LazyLock;

use mimalloc::MiMalloc;
use reqwest::Client;
use tokio::select;

mod display;
mod image;
mod openweather;
mod settings;
mod switchbot;
mod wallpaper;

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

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

static REST_CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

#[tokio::main(worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    logger::init();

    // Stop the whole program if any worker finishes or fails.
    // Each worker is expected to run until it returns an error or is shut down.
    select! {
        result = switchbot::worker() => result,
        result = openweather::worker() => result,
        result = wallpaper::worker() => result,
        result = display::worker() => result,
    }
}
