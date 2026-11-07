//! Modos de ping: cada modo decide QUANDO e O QUE responder a partir dos
//! eventos do canal observado. O envio passa sempre pelo `ReplyGate`
//! (status, sleep e dedupe são gerais).
//!
//! Para criar um modo novo:
//! 1. variante em `config::ModeKind` (+ settings em `config::ModesConfig`);
//! 2. módulo em `core/modes/<modo>/` implementando `Mode`;
//! 3. registrar em `create()`;
//! 4. campos na UI (`ui/app.js`, objeto `MODES`).

pub mod thread;

use crate::config::{Config, ModeKind, Status};
use crate::core::reply::{BotEvent, Notify, ReplyGate, Sender};
use std::sync::{Arc, RwLock};

/// Novo tópico criado (THREAD_CREATE).
#[derive(Debug, Clone)]
pub struct ThreadEvent {
    pub id: String,
    pub parent_id: Option<String>,
    pub owner_id: Option<String>,
    pub newly_created: bool,
    pub name: Option<String>,
}

/// Mensagem criada (MESSAGE_CREATE), já reduzida ao necessário.
#[derive(Debug, Clone, Copy)]
pub struct MessageEvent<'a> {
    pub channel_id: &'a str,
    pub author_id: &'a str,
    pub content: &'a str,
}

pub trait Mode: Send + Sync {
    fn kind(&self) -> ModeKind;
    fn on_thread_create(self: Arc<Self>, _ev: &ThreadEvent) {}
    fn on_message_create(self: Arc<Self>, _msg: &MessageEvent) {}
    /// Aplica config nova ao vivo (canal, settings do modo).
    fn apply(&self, cfg: &Config);
    /// Cancela tudo que está pendente. Retorna quantos itens foram cancelados.
    fn cancel_all(&self) -> usize;
}

pub fn create(kind: ModeKind, cfg: &Config, gate: Arc<ReplyGate>) -> Arc<dyn Mode> {
    match kind {
        ModeKind::Thread => thread::ThreadMode::new(cfg, gate),
    }
}

/// Mantém o modo ativo e encaminha eventos/controles para ele.
pub struct ModeRunner {
    gate: Arc<ReplyGate>,
    active: RwLock<Arc<dyn Mode>>,
}

impl ModeRunner {
    pub fn new(cfg: &Config, sender: Arc<dyn Sender>, notify: Notify) -> Arc<Self> {
        let gate = ReplyGate::new(cfg.status, cfg.sleep_after_reply, sender, notify);
        let runner = Arc::new(Self { gate: gate.clone(), active: RwLock::new(create(cfg.mode, cfg, gate.clone())) });
        let weak = Arc::downgrade(&runner);
        gate.set_on_block(move || {
            if let Some(r) = weak.upgrade() {
                r.cancel_pending();
            }
        });
        runner
    }

    fn active(&self) -> Arc<dyn Mode> {
        self.active.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn kind(&self) -> ModeKind {
        self.active().kind()
    }

    pub fn status(&self) -> Status {
        self.gate.status()
    }

    // ── Eventos (caminho quente) ────────────────────────────────────

    pub fn on_thread_create(&self, ev: &ThreadEvent) {
        if self.gate.is_on() {
            self.active().on_thread_create(ev);
        }
    }

    pub fn on_message_create(&self, msg: &MessageEvent) {
        if self.gate.is_on() {
            self.active().on_message_create(msg);
        }
    }

    // ── Controle ────────────────────────────────────────────────────

    pub fn set_status(&self, status: Status) {
        self.gate.set_status(status);
    }

    fn cancel_pending(&self) {
        let n = self.active().cancel_all();
        if n > 0 {
            self.gate.notify(BotEvent::Info(format!("{n} pendente(s) cancelado(s)")));
        }
    }

    /// Aplica config ao vivo; troca de modo cancela o anterior.
    pub fn apply(&self, cfg: &Config) {
        self.gate.set_sleep_after_reply(cfg.sleep_after_reply);
        let current = self.active();
        if current.kind() != cfg.mode {
            current.cancel_all();
            *self.active.write().unwrap_or_else(|e| e.into_inner()) = create(cfg.mode, cfg, self.gate.clone());
            self.gate.notify(BotEvent::Info(format!("Modo alterado para {:?}", cfg.mode)));
        } else {
            current.apply(cfg);
        }
    }
}
