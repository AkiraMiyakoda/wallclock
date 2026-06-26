// Copyright © 2026 Akira Miyakoda
//
// This software is released under the MIT License.
// https://opensource.org/licenses/MIT

use std::time::Duration;

use anyhow::bail;
use base64::prelude::*;
use chrono::TimeDelta;
use chrono::Utc;
use log::error;
use log::info;
use reqwest::RequestBuilder;
use reqwest::Url;
use reqwest::header;
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use tokio::time::interval;
use tokio::try_join;
use uuid::Uuid;

use crate::REST_CLIENT;
use crate::Update;
use crate::settings;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_field_names)]
struct Message {
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
    pub humidity: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct SwitchBotData {
    pub indoor: WoIOSensor,
    pub outdoor: WoIOSensor,
    pub tank: WoIOSensor,
}

pub(super) async fn worker(sender: &mpsc::Sender<Update>) -> anyhow::Result<()> {
    let mut interval = interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut last_tick: i64 = -1;

    loop {
        interval.tick().await;

        let tick = (Utc::now().timestamp() - 10) / TimeDelta::minutes(3).num_seconds();
        if tick == last_tick {
            continue;
        }

        match inquire().await {
            Ok(data) => {
                info!("Switchbot updated");
                sender.send(Update::SwitchBot(data)).await?;
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

    let settings::Switchbot { devices, token, secret } = settings::switchbot();

    let base_url = Url::parse(BASE_URL)?;

    let (indoor, outdoor, tank) = try_join!(
        fetch_device_status(base_url.clone(), &devices.0.id, token, secret),
        fetch_device_status(base_url.clone(), &devices.1.id, token, secret),
        fetch_device_status(base_url, &devices.2.id, token, secret),
    )?;

    Ok(SwitchBotData { indoor, outdoor, tank })
}

async fn fetch_device_status(base_url: Url, device_id: &str, token: &str, secret: &str) -> anyhow::Result<WoIOSensor> {
    let url = base_url.join(&format!("devices/{device_id}/status"))?;

    let res = REST_CLIENT
        .get(url)
        .auth_headers(token, secret)
        .header(header::CONTENT_TYPE, mime::APPLICATION_JSON.as_ref())
        .send()
        .await?;

    let msg: Message = res.json().await?;
    if msg.status_code != 100 {
        bail!("Switchbot API error: {} {}", msg.status_code, msg.message);
    }

    let Some(Body::WoIOSensor(body)) = msg.body else {
        bail!("Invalid message format");
    };

    anyhow::Ok(body)
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
