//! Cliente mínimo do Gateway do Discord para conta de usuário (port do
//! gateway.js): HELLO/heartbeat/ACK, IDENTIFY, RESUME, RECONNECT (op 7),
//! INVALID_SESSION (op 9), backoff, close codes fatais, op 37 (subscribe).
//!
//! Parse preguiçoso: cada frame é lido como `{op, s, t, d: RawValue}`; o
//! `d` só é desserializado pelo handler para os eventos que interessam.

use super::properties::{self, API_VERSION, CAPABILITIES};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, value::RawValue};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::time::{sleep, sleep_until, Instant};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;

const DEFAULT_URL: &str = "wss://gateway.discord.gg";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnState {
    NoToken,
    Connecting,
    Connected,
    Reconnecting,
    InvalidToken,
    Error,
}

pub trait Handler: Send + Sync + 'static {
    fn on_dispatch(&self, t: &str, d: &RawValue);
    fn on_connection(&self, state: ConnState);
}

enum Command {
    Subscribe { guild_id: String, channel_id: String },
    Shutdown,
}

pub struct GatewayHandle {
    tx: mpsc::UnboundedSender<Command>,
}

impl GatewayHandle {
    pub fn subscribe(&self, guild_id: &str, channel_id: &str) {
        let _ = self.tx.send(Command::Subscribe { guild_id: guild_id.into(), channel_id: channel_id.into() });
    }
    pub fn shutdown(&self) {
        let _ = self.tx.send(Command::Shutdown);
    }
}

impl Drop for GatewayHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
    }
}

pub fn spawn(token: String, handler: Arc<dyn Handler>) -> GatewayHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(run(token, handler, rx));
    GatewayHandle { tx }
}

#[derive(Deserialize)]
struct Frame<'a> {
    op: u8,
    #[serde(default)]
    s: Option<u64>,
    #[serde(default, borrow)]
    t: Option<&'a str>,
    #[serde(default, borrow)]
    d: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct Hello {
    heartbeat_interval: u64,
}

#[derive(Deserialize)]
struct ReadySession {
    session_id: String,
    #[serde(default)]
    resume_gateway_url: Option<String>,
}

#[derive(Default)]
struct Session {
    id: Option<String>,
    resume_url: Option<String>,
    seq: Option<u64>,
}

impl Session {
    fn clear(&mut self) {
        *self = Session::default();
    }
    fn resumable(&self) -> bool {
        self.id.is_some() && self.seq.is_some()
    }
}

enum Outcome {
    Retry(Option<Duration>),
    Fatal(u16, &'static str),
    Shutdown,
}

fn fatal_reason(code: u16) -> Option<&'static str> {
    match code {
        4004 => Some("Token inválido (Authentication failed)"),
        4010 => Some("Shard inválido"),
        4011 => Some("Sharding necessário"),
        4012 => Some("Versão de API inválida"),
        4013 => Some("Intents inválidos"),
        4014 => Some("Intents não permitidos"),
        _ => None,
    }
}

fn jitter(max_ms: u64) -> u64 {
    let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos() as u64).unwrap_or(0);
    if max_ms == 0 { 0 } else { n % max_ms }
}

fn backoff(attempts: u32) -> Duration {
    let base = if attempts == 0 { 0 } else { (1000u64 << (attempts - 1).min(5)).min(30_000) };
    Duration::from_millis(base + jitter(500))
}

async fn run(token: String, handler: Arc<dyn Handler>, mut rx: mpsc::UnboundedReceiver<Command>) {
    let mut session = Session::default();
    let mut attempts: u32 = 0;

    loop {
        handler.on_connection(if attempts == 0 { ConnState::Connecting } else { ConnState::Reconnecting });
        let base = session.resume_url.clone().filter(|_| session.resumable()).unwrap_or_else(|| DEFAULT_URL.into());
        let url = format!("{}/?v={API_VERSION}&encoding=json", base.trim_end_matches('/'));

        let outcome = match connect(&url).await {
            Ok(ws) => run_session(ws, &token, &handler, &mut session, &mut attempts, &mut rx).await,
            Err(e) => {
                crate::logf!("Falha ao conectar no gateway: {e}");
                Outcome::Retry(None)
            }
        };

        match outcome {
            Outcome::Shutdown => return,
            Outcome::Fatal(code, reason) => {
                crate::logf!("Erro fatal no gateway ({code}): {reason}");
                handler.on_connection(if code == 4004 { ConnState::InvalidToken } else { ConnState::Error });
                return;
            }
            Outcome::Retry(delay) => {
                let delay = delay.unwrap_or_else(|| backoff(attempts));
                attempts = attempts.saturating_add(1);
                handler.on_connection(ConnState::Reconnecting);
                crate::logf!("Reconectando ao gateway em {}ms...", delay.as_millis());
                tokio::select! {
                    _ = sleep(delay) => {}
                    cmd = rx.recv() => match cmd {
                        Some(Command::Subscribe { .. }) => {} // reenviado no próximo READY
                        Some(Command::Shutdown) | None => return,
                    }
                }
            }
        }
    }
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(url: &str) -> Result<Ws, String> {
    let mut req = url.into_client_request().map_err(|e| e.to_string())?;
    let h = req.headers_mut();
    h.insert("User-Agent", HeaderValue::from_static(properties::user_agent()));
    h.insert("Origin", HeaderValue::from_static("https://discord.com"));
    let (ws, _) = tokio_tungstenite::connect_async(req).await.map_err(|e| e.to_string())?;
    Ok(ws)
}

async fn run_session(
    ws: Ws,
    token: &str,
    handler: &Arc<dyn Handler>,
    session: &mut Session,
    attempts: &mut u32,
    rx: &mut mpsc::UnboundedReceiver<Command>,
) -> Outcome {
    let (mut write, mut read) = ws.split();
    let mut interval: Option<Duration> = None;
    let mut next_hb = Instant::now() + Duration::from_secs(3600);
    let mut acked = true;

    macro_rules! send_json {
        ($v:expr) => {
            if write.send(Message::Text($v.to_string().into())).await.is_err() {
                return Outcome::Retry(None);
            }
        };
    }

    loop {
        tokio::select! {
            biased;

            msg = read.next() => {
                let msg = match msg {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => {
                        crate::logf!("Erro na conexão do gateway: {e}");
                        return Outcome::Retry(None);
                    }
                    None => return Outcome::Retry(None),
                };
                match msg {
                    Message::Text(txt) => {
                        let Ok(frame) = serde_json::from_str::<Frame>(txt.as_str()) else { continue };
                        match frame.op {
                            0 => {
                                if let Some(s) = frame.s {
                                    session.seq = Some(s);
                                }
                                let (Some(t), Some(d)) = (frame.t, frame.d) else { continue };
                                match t {
                                    "READY" => {
                                        if let Ok(r) = serde_json::from_str::<ReadySession>(d.get()) {
                                            session.id = Some(r.session_id);
                                            session.resume_url = r.resume_gateway_url;
                                        }
                                        *attempts = 0;
                                        handler.on_connection(ConnState::Connected);
                                    }
                                    "RESUMED" => {
                                        *attempts = 0;
                                        crate::logf!("Sessão do gateway retomada");
                                        handler.on_connection(ConnState::Connected);
                                    }
                                    _ => {}
                                }
                                handler.on_dispatch(t, d);
                            }
                            10 => {
                                let Some(hello) = frame.d.and_then(|d| serde_json::from_str::<Hello>(d.get()).ok()) else { continue };
                                let iv = Duration::from_millis(hello.heartbeat_interval);
                                interval = Some(iv);
                                acked = true;
                                next_hb = Instant::now() + Duration::from_millis(jitter(hello.heartbeat_interval));
                                if session.resumable() {
                                    send_json!(json!({ "op": 6, "d": {
                                        "token": token,
                                        "session_id": session.id,
                                        "seq": session.seq
                                    }}));
                                } else {
                                    send_json!(json!({ "op": 2, "d": {
                                        "token": token,
                                        "capabilities": CAPABILITIES,
                                        "properties": properties::properties(),
                                        "presence": { "status": "unknown", "since": 0, "activities": [], "afk": false },
                                        "compress": false,
                                        "client_state": { "guild_versions": {} }
                                    }}));
                                }
                            }
                            11 => acked = true,
                            1 => send_json!(json!({ "op": 1, "d": session.seq })),
                            7 => {
                                crate::logf!("Gateway pediu reconexão (op 7)");
                                let _ = write.send(Message::Close(Some(CloseFrame { code: CloseCode::from(4000), reason: "".into() }))).await;
                                return Outcome::Retry(Some(Duration::from_millis(jitter(500))));
                            }
                            9 => {
                                let resumable = frame.d.map(|d| d.get() == "true").unwrap_or(false);
                                if !resumable {
                                    session.clear();
                                }
                                crate::logf!("Sessão inválida (op 9, retomável: {resumable})");
                                return Outcome::Retry(Some(Duration::from_millis(1000 + jitter(4000))));
                            }
                            _ => {}
                        }
                    }
                    Message::Close(frame) => {
                        let code = frame.as_ref().map(|f| u16::from(f.code)).unwrap_or(1006);
                        if let Some(reason) = fatal_reason(code) {
                            return Outcome::Fatal(code, reason);
                        }
                        if code == 4007 || code == 4009 {
                            session.clear();
                        }
                        crate::logf!("Gateway fechou a conexão (code {code})");
                        return Outcome::Retry(None);
                    }
                    _ => {}
                }
            }

            _ = sleep_until(next_hb), if interval.is_some() => {
                if !acked {
                    crate::logf!("Heartbeat sem resposta - reconectando");
                    return Outcome::Retry(None);
                }
                acked = false;
                send_json!(json!({ "op": 1, "d": session.seq }));
                next_hb = Instant::now() + interval.unwrap_or(Duration::from_secs(40));
            }

            cmd = rx.recv() => match cmd {
                Some(Command::Subscribe { guild_id, channel_id }) => {
                    let mut channels = serde_json::Map::new();
                    channels.insert(channel_id, json!([[0, 99]]));
                    let mut subs = serde_json::Map::new();
                    subs.insert(guild_id, json!({
                        "typing": true,
                        "threads": true,
                        "activities": true,
                        "member_updates": false,
                        "channels": channels
                    }));
                    send_json!(json!({ "op": 37, "d": { "subscriptions": subs } }));
                }
                Some(Command::Shutdown) | None => {
                    let _ = write.send(Message::Close(Some(CloseFrame { code: CloseCode::Normal, reason: "".into() }))).await;
                    return Outcome::Shutdown;
                }
            }
        }
    }
}
