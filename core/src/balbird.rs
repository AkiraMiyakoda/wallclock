// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Duration;

use arc_swap::ArcSwapOption;
use chrono::TimeDelta;
use chrono::Utc;
use log::error;
use log::info;
use serde::Deserialize;
use tokio::time::MissedTickBehavior;
use tokio::time::interval;

use crate::REST_CLIENT;
use crate::settings;

#[derive(Debug, Deserialize)]
struct BalbirdResponse {
    #[allow(dead_code)]
    system: SystemStatus,
    database: DatabaseStatus,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct SystemStatus {
    used_memory_bytes: u64,
    total_memory_bytes: u64,
    used_swap_bytes: u64,
    total_swap_bytes: u64,
    used_disk_bytes: u64,
    total_disk_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct DatabaseStatus {
    tables: Vec<TableStatus>,
}

#[derive(Debug, Deserialize)]
struct TableStatus {
    name: String,
    #[allow(dead_code)]
    size_bytes: u64,
    age_seconds: Option<u64>,
}

#[derive(Debug)]
pub struct BalbirdData {
    pub is_healthy: bool,
}

static LATEST_DATA: LazyLock<ArcSwapOption<BalbirdData>> = LazyLock::new(|| ArcSwapOption::from(None));

pub fn get_latest() -> Option<Arc<BalbirdData>> {
    LATEST_DATA.load_full()
}

pub async fn worker() -> anyhow::Result<()> {
    let mut interval = interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut last_tick: i64 = -1;

    loop {
        interval.tick().await;

        // Run about 30 seconds after each 3-minute boundary,
        // so this worker does not run at the same time as the others.
        let tick = (Utc::now().timestamp() - 30) / TimeDelta::minutes(3).num_seconds();
        if tick == last_tick {
            continue;
        }

        match inquire().await {
            Ok(data) => {
                LATEST_DATA.store(Some(Arc::new(data)));
                info!("Balbird updated");
            }
            Err(e) => {
                LATEST_DATA.store(None);
                error!("Failed to update Balbird data: {e:?}");
            }
        }

        // Update this even on failure to avoid retrying every second.
        last_tick = tick;
    }
}

async fn inquire() -> anyhow::Result<BalbirdData> {
    const URL: &str = "https://api.balancingbird.net/api/1/status";

    let settings::Balbird { api_key } = settings::balbird();

    let response: BalbirdResponse = REST_CLIENT
        .get(URL)
        .bearer_auth(api_key)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // Ignore `transactions`; it is updated only when trades occur.
    // No age means the service is not ready yet.
    let max_age = response
        .database
        .tables
        .iter()
        .filter(|table| table.name != "transactions")
        .filter_map(|table| table.age_seconds)
        .max();
    let is_healthy = max_age.map_or(false, |age| age < 60);

    Ok(BalbirdData { is_healthy })
}
