//! Cliente REST do Discord. `send_message` é o caminho quente: um único
//! `reqwest::Client` com conexões keep-alive pré-aquecidas.

use super::properties::{self, API_VERSION, LOCALE};
use crate::core::bot::{SendFuture, Sender};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, ORIGIN, USER_AGENT};
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const BASE: &str = "https://discord.com/api/v";
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelInfo {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub kind: u8,
    #[serde(default)]
    pub guild_id: Option<String>,
}

#[derive(Debug)]
pub struct RestError {
    pub status: u16,
    pub code: Option<u64>,
    pub message: String,
}

impl std::fmt::Display for RestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}", self.status)?;
        if !self.message.is_empty() {
            write!(f, " - {}", self.message)?;
        }
        Ok(())
    }
}

#[derive(Deserialize, Default)]
struct ErrorBody {
    #[serde(default)]
    code: Option<u64>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    retry_after: Option<f64>,
}

pub struct Rest {
    client: reqwest::Client,
    base: String,
}

fn nonce() -> String {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    ((ms.saturating_sub(DISCORD_EPOCH_MS)) << 22).to_string()
}

impl Rest {
    pub fn new(token: &str) -> Result<Self, String> {
        let mut h = HeaderMap::new();
        let mut auth = HeaderValue::from_str(token).map_err(|_| "Token com caracteres inválidos".to_string())?;
        auth.set_sensitive(true);
        h.insert(AUTHORIZATION, auth);
        h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        h.insert(USER_AGENT, HeaderValue::from_static(properties::user_agent()));
        h.insert("x-super-properties", HeaderValue::from_static(properties::super_properties()));
        h.insert("x-discord-locale", HeaderValue::from_static(LOCALE));
        h.insert(ORIGIN, HeaderValue::from_static("https://discord.com"));

        let client = reqwest::Client::builder()
            .default_headers(h)
            .pool_idle_timeout(None)
            .pool_max_idle_per_host(10)
            .tcp_keepalive(Duration::from_secs(30))
            .tcp_nodelay(true)
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| format!("Falha ao criar cliente HTTP: {e}"))?;
        Ok(Self { client, base: format!("{BASE}{API_VERSION}") })
    }

    async fn error_from(resp: reqwest::Response) -> (RestError, Option<f64>) {
        let status = resp.status().as_u16();
        let body: ErrorBody = resp.json().await.unwrap_or_default();
        (
            RestError { status, code: body.code, message: body.message.unwrap_or_default() },
            body.retry_after,
        )
    }

    /// Envia mensagem; em 429 aguarda `retry_after` e tenta mais 1x.
    pub async fn send_message(&self, channel_id: &str, content: &str) -> Result<(), RestError> {
        let url = format!("{}/channels/{channel_id}/messages", self.base);
        for attempt in 0..2 {
            let body = json!({ "content": content, "nonce": nonce(), "tts": false, "flags": 0 });
            let resp = self.client.post(&url).json(&body).send().await.map_err(|e| RestError {
                status: 0,
                code: None,
                message: e.without_url().to_string(),
            })?;
            if resp.status().is_success() {
                let _ = resp.bytes().await; // devolve a conexão ao pool
                return Ok(());
            }
            let (err, retry_after) = Self::error_from(resp).await;
            if err.status == 429 && attempt == 0 {
                let wait = Duration::from_secs_f64(retry_after.unwrap_or(1.0).clamp(0.0, 10.0));
                crate::logf!("Rate limit (429). Tentando de novo em {}ms", wait.as_millis());
                tokio::time::sleep(wait).await;
                continue;
            }
            return Err(err);
        }
        unreachable!()
    }

    pub async fn get_channel(&self, channel_id: &str) -> Result<ChannelInfo, RestError> {
        let url = format!("{}/channels/{channel_id}", self.base);
        let resp = self.client.get(&url).send().await.map_err(|e| RestError {
            status: 0,
            code: None,
            message: e.without_url().to_string(),
        })?;
        if resp.status().is_success() {
            return resp.json().await.map_err(|e| RestError { status: 0, code: None, message: e.to_string() });
        }
        Err(Self::error_from(resp).await.0)
    }

    /// Mantém a conexão TLS aberta (chamado periodicamente).
    pub async fn ping(&self) {
        if let Ok(r) = self.client.get(format!("{}/gateway", self.base)).send().await {
            let _ = r.bytes().await;
        }
    }
}

/// Sender do Bot apontando para o `Rest` atual (troca ao vivo quando o token muda).
#[derive(Clone, Default)]
pub struct SharedRest(pub Arc<RwLock<Option<Arc<Rest>>>>);

impl SharedRest {
    pub fn get(&self) -> Option<Arc<Rest>> {
        self.0.read().ok().and_then(|g| g.clone())
    }
    pub fn set(&self, rest: Option<Arc<Rest>>) {
        if let Ok(mut g) = self.0.write() {
            *g = rest;
        }
    }
}

impl Sender for SharedRest {
    fn send(&self, channel_id: String, content: String) -> SendFuture {
        let rest = self.get();
        Box::pin(async move {
            let rest = rest.ok_or_else(|| "sem conexão REST (token ausente)".to_string())?;
            rest.send_message(&channel_id, &content).await.map_err(|e| e.to_string())
        })
    }
}
