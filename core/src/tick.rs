// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::time::Duration;

use chrono::Utc;
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use tokio::time::interval;

use crate::Update;

pub async fn worker(sender: &mpsc::Sender<Update>) -> anyhow::Result<()> {
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
