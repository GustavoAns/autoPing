//! ReplyGate: ponto único de envio para TODOS os modos de ping.
//!
//! Centraliza as regras gerais de resposta, para que qualquer modo novo as
//! herde automaticamente:
//! - status: só envia com status On (Off/Sleep bloqueiam);
//! - dedupe: não responde o mesmo canal/tópico duas vezes em 30s;
//! - sleep após responder: ao disparar uma resposta entra em Sleep e cancela
//!   o que o modo tinha pendente; se o envio falhar, volta para On.

use crate::config::Status;
use serde::Serialize;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, Duration, Instant};

const DEDUPE_TTL: Duration = Duration::from_secs(30);

pub type SendFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

/// Abstração do envio (REST em produção, mock nos testes).
pub trait Sender: Send + Sync + 'static {
    fn send(&self, channel_id: String, content: String) -> SendFuture;
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    pub thread_id: String,
    pub response_ms: u64,
    pub reason: &'static str,
    pub at_unix_ms: u64,
}

#[derive(Debug, Clone)]
pub enum BotEvent {
    StatusChanged(Status),
    Replied(Reply),
    Info(String),
}

pub type Notify = Arc<dyn Fn(BotEvent) + Send + Sync>;

struct GateState {
    status: Status,
    sleep_after_reply: bool,
    processed: HashSet<String>,
}

pub struct ReplyGate {
    state: Mutex<GateState>,
    sender: Arc<dyn Sender>,
    notify: Notify,
    /// Chamado quando o status deixa de ser On (cancela pendentes do modo).
    on_block: OnceLock<Box<dyn Fn() + Send + Sync>>,
}

pub fn unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl ReplyGate {
    pub fn new(status: Status, sleep_after_reply: bool, sender: Arc<dyn Sender>, notify: Notify) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(GateState { status, sleep_after_reply, processed: HashSet::new() }),
            sender,
            notify,
            on_block: OnceLock::new(),
        })
    }

    pub fn set_on_block(&self, f: impl Fn() + Send + Sync + 'static) {
        let _ = self.on_block.set(Box::new(f));
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn notify(&self, ev: BotEvent) {
        (self.notify)(ev);
    }

    pub fn status(&self) -> Status {
        self.lock().status
    }

    pub fn is_on(&self) -> bool {
        self.lock().status == Status::On
    }

    pub fn was_replied(&self, id: &str) -> bool {
        self.lock().processed.contains(id)
    }

    pub fn set_sleep_after_reply(&self, v: bool) {
        self.lock().sleep_after_reply = v;
    }

    /// Muda o status; retorna false se já estava nele.
    pub fn set_status(&self, status: Status) -> bool {
        {
            let mut g = self.lock();
            if g.status == status {
                return false;
            }
            g.status = status;
        }
        if status != Status::On {
            if let Some(f) = self.on_block.get() {
                f();
            }
        }
        self.notify(BotEvent::StatusChanged(status));
        true
    }

    /// Envia `content` em `channel_id` respeitando status, dedupe e sleep.
    pub async fn send(self: Arc<Self>, channel_id: String, content: String, start: Instant, reason: &'static str) -> bool {
        let sleeping = {
            let mut g = self.lock();
            if g.status != Status::On || content.is_empty() {
                drop(g);
                self.notify(BotEvent::Info(format!("Envio cancelado para {channel_id} (desligado)")));
                return false;
            }
            if !g.processed.insert(channel_id.clone()) {
                return false;
            }
            let sleeping = g.sleep_after_reply;
            if sleeping {
                g.status = Status::Sleep; // bloqueia envios concorrentes já agora
            }
            sleeping
        };
        if sleeping {
            if let Some(f) = self.on_block.get() {
                f();
            }
            self.notify(BotEvent::StatusChanged(Status::Sleep));
        }

        match self.sender.send(channel_id.clone(), content).await {
            Ok(()) => {
                self.notify(BotEvent::Replied(Reply {
                    thread_id: channel_id.clone(),
                    response_ms: start.elapsed().as_millis() as u64,
                    reason,
                    at_unix_ms: unix_ms(),
                }));
                let gate = Arc::clone(&self);
                tokio::spawn(async move {
                    sleep(DEDUPE_TTL).await;
                    gate.lock().processed.remove(&channel_id);
                });
                true
            }
            Err(e) => {
                let reverted = {
                    let mut g = self.lock();
                    g.processed.remove(&channel_id);
                    if sleeping && g.status == Status::Sleep {
                        g.status = Status::On;
                        true
                    } else {
                        false
                    }
                };
                self.notify(BotEvent::Info(format!("Erro ao enviar em {channel_id}: {e}")));
                if reverted {
                    self.notify(BotEvent::StatusChanged(Status::On));
                }
                false
            }
        }
    }
}
