// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::sync::LazyLock;
use std::time::Duration;

use anyhow::bail;
use base64::prelude::*;
use chrono::TimeDelta;
use chrono::Utc;
use log::error;
use log::info;
use reqwest::RequestBuilder;
use reqwest::Url;
use serde::Deserialize;
use tokio::sync::RwLock;
use tokio::time::MissedTickBehavior;
use tokio::time::interval;
use tokio::try_join;
use uuid::Uuid;

use crate::REST_CLIENT;
use crate::settings;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwitchBotResponse {
    status_code: i32,
    message: String,
    body: Option<Body>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "deviceType")]
enum Body {
    WoIOSensor(WoIOSensor),
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct WoIOSensor {
    pub temperature: f32,
    pub humidity: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct SwitchBotData {
    pub indoor: WoIOSensor,
    pub outdoor: WoIOSensor,
    pub tank: WoIOSensor,
}

static LATEST_DATA: LazyLock<RwLock<Option<SwitchBotData>>> = LazyLock::new(|| RwLock::new(None));

pub async fn get_latest() -> Option<SwitchBotData> {
    *LATEST_DATA.read().await
}

pub async fn worker() -> anyhow::Result<()> {
    let mut interval = interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut last_tick: i64 = -1;

    loop {
        interval.tick().await;

        // Run about 10 seconds after each 3-minute boundary to stagger this worker.
        let tick = (Utc::now().timestamp() - 10) / TimeDelta::minutes(3).num_seconds();
        if tick == last_tick {
            continue;
        }

        match inquire().await {
            Ok(data) => {
                *LATEST_DATA.write().await = Some(data);

                info!("SwitchBot updated");
            }
            Err(e) => {
                error!("Failed to update SwitchBot data: {e:?}");
            }
        }

        last_tick = tick;
    }
}

async fn inquire() -> anyhow::Result<SwitchBotData> {
    const BASE_URL: &str = "https://api.switch-bot.com/v1.1/";

    let settings::SwitchBot { devices, token, secret } = settings::switchbot();

    let base_url = Url::parse(BASE_URL)?;

    let inner_task = async |device_id: &str| {
        let url = base_url.join(&format!("devices/{device_id}/status"))?;
        let response: SwitchBotResponse = REST_CLIENT
            .get(url)
            .auth_headers(token, secret)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if response.status_code != 100 {
            bail!(
                "SwitchBot API error for device {device_id}: {} {}",
                response.status_code,
                response.message
            );
        }
        let Some(Body::WoIOSensor(body)) = response.body else {
            bail!("SwitchBot API did not return WoIOSensor data for device {device_id}");
        };

        anyhow::Ok(body)
    };

    let (indoor, outdoor, tank) = try_join!(
        inner_task(&devices.indoor.id),
        inner_task(&devices.outdoor.id),
        inner_task(&devices.tank.id),
    )?;

    Ok(SwitchBotData { indoor, outdoor, tank })
}

trait AuthHeaders {
    fn auth_headers(self, token: &str, secret: &str) -> Self;
}

impl AuthHeaders for RequestBuilder {
    fn auth_headers(self, token: &str, secret: &str) -> Self {
        let nonce = Uuid::new_v4().to_string();
        let t = Utc::now().timestamp_millis().to_string();
        let sign = {
            let mut hmac = hmac_sha256::HMAC::new(secret.as_bytes());
            hmac.update(token.as_bytes());
            hmac.update(t.as_bytes());
            hmac.update(nonce.as_bytes());
            BASE64_STANDARD.encode(hmac.finalize())
        };

        self.header("Authorization", token)
            .header("t", t)
            .header("nonce", nonce)
            .header("sign", sign)
    }
}
