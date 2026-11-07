//! Bot: decide QUANDO enviar a mensagem automática (port do responder Node).
//!
//! Regras de negócio:
//! - Nunca envia no THREAD_CREATE. O gatilho é a 1ª msg do criador dentro do
//!   tópico (o Discord cria o tópico e só depois posta a msg inicial).
//! - Sem msg do criador em `wait_for_message_ms` -> envia mesmo assim.
//! - Modo humano: espera `humanized_delay_ms` após a msg do criador.
//! - Status Off/Sleep bloqueia qualquer envio e cancela pendentes.
//! - Sleep após responder: ao disparar uma resposta entra em Sleep (cancela
//!   pendentes). Se o envio falhar, volta para On.

use crate::config::{Config, Status};
use crate::core::humanized::{Humanized, ThreadDecision};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::task::AbortHandle;
use tokio::time::{sleep, sleep_until, Duration, Instant};

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

#[derive(Debug, Clone)]
pub struct ThreadEvent {
    pub id: String,
    pub parent_id: Option<String>,
    pub owner_id: Option<String>,
    pub newly_created: bool,
    pub name: Option<String>,
}

struct Inner {
    cfg: Config,
    waiter: Humanized,
    timers: HashMap<String, AbortHandle>,
    processed: HashSet<String>,
}

impl Inner {
    fn cancel_all(&mut self) -> usize {
        let n = self.waiter.pending_count().max(self.timers.len());
        for (_, h) in self.timers.drain() {
            h.abort();
        }
        self.waiter.clear();
        n
    }
}

type Notify = Arc<dyn Fn(BotEvent) + Send + Sync>;

pub struct Bot {
    inner: Mutex<Inner>,
    sender: Arc<dyn Sender>,
    notify: Notify,
}

impl Bot {
    pub fn new(cfg: Config, sender: Arc<dyn Sender>, notify: Notify) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                cfg,
                waiter: Humanized::default(),
                timers: HashMap::new(),
                processed: HashSet::new(),
            }),
            sender,
            notify,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn config(&self) -> Config {
        self.lock().cfg.clone()
    }

    pub fn status(&self) -> Status {
        self.lock().cfg.status
    }

    // ── Eventos do gateway (caminho quente) ─────────────────────────

    pub fn on_thread_create(self: &Arc<Self>, ev: &ThreadEvent) {
        let start = Instant::now();
        let mut g = self.lock();
        if g.cfg.status != Status::On || g.cfg.auto_message.is_empty() {
            return;
        }
        if !ev.newly_created || ev.parent_id.as_deref() != Some(g.cfg.channel_id.as_str()) {
            return;
        }
        if g.processed.contains(&ev.id) || g.timers.contains_key(&ev.id) || g.waiter.is_pending(&ev.id) {
            return;
        }
        let owner = ev.owner_id.clone().unwrap_or_default();
        let waiting = match g.waiter.on_thread_create(&ev.id, &owner, start) {
            ThreadDecision::OwnerAlreadyPosted => {
                self.schedule_send(&mut g, ev.id.clone(), start, "owner-message-early");
                false
            }
            ThreadDecision::Wait => {
                let deadline = start + Duration::from_millis(g.cfg.wait_for_message_ms);
                let bot = Arc::clone(self);
                let id = ev.id.clone();
                let h = tokio::spawn(async move {
                    sleep_until(deadline).await;
                    bot.on_timeout(id).await;
                });
                g.timers.insert(ev.id.clone(), h.abort_handle());
                true
            }
        };
        let wait_ms = g.cfg.wait_for_message_ms;
        drop(g);
        let name = ev.name.as_deref().unwrap_or("?");
        (self.notify)(BotEvent::Info(if waiting {
            format!("Novo tópico \"{name}\" ({}) - aguardando 1ª msg do criador (até {wait_ms}ms)", ev.id)
        } else {
            format!("Novo tópico \"{name}\" ({}) - criador já postou", ev.id)
        }));
    }

    pub fn on_message_create(self: &Arc<Self>, channel_id: &str, author_id: &str) {
        let mut g = self.lock();
        if g.cfg.status != Status::On {
            return;
        }
        if let Some(start) = g.waiter.on_message_create(channel_id, author_id, Instant::now()) {
            if let Some(h) = g.timers.remove(channel_id) {
                h.abort(); // cancela o timeout
            }
            self.schedule_send(&mut g, channel_id.to_string(), start, "owner-message");
        }
    }

    fn schedule_send(self: &Arc<Self>, g: &mut Inner, thread_id: String, start: Instant, reason: &'static str) {
        let delay = g.cfg.effective_delay_ms();
        let bot = Arc::clone(self);
        if delay == 0 {
            tokio::spawn(async move { bot.send(thread_id, start, reason).await });
            return;
        }
        let id = thread_id.clone();
        let deadline = Instant::now() + Duration::from_millis(delay);
        let h = tokio::spawn(async move {
            sleep_until(deadline).await;
            bot.send(id, start, reason).await;
        });
        g.timers.insert(thread_id, h.abort_handle());
    }

    async fn on_timeout(self: Arc<Self>, thread_id: String) {
        let start = {
            let mut g = self.lock();
            g.timers.remove(&thread_id);
            g.waiter.take_pending(&thread_id).map(|p| p.start)
        };
        if let Some(start) = start {
            (self.notify)(BotEvent::Info(format!("Timeout em {thread_id} - enviando mesmo assim")));
            self.send(thread_id, start, "timeout").await;
        }
    }

    async fn send(self: Arc<Self>, thread_id: String, start: Instant, reason: &'static str) {
        let (content, sleeping) = {
            let mut g = self.lock();
            g.timers.remove(&thread_id); // este timer já disparou
            if g.cfg.status != Status::On || g.cfg.auto_message.is_empty() {
                drop(g);
                (self.notify)(BotEvent::Info(format!("Envio cancelado para {thread_id} (desligado)")));
                return;
            }
            if !g.processed.insert(thread_id.clone()) {
                return;
            }
            let sleeping = g.cfg.sleep_after_reply;
            if sleeping {
                g.cfg.status = Status::Sleep;
                g.cancel_all();
            }
            (g.cfg.auto_message.clone(), sleeping)
        };
        if sleeping {
            (self.notify)(BotEvent::StatusChanged(Status::Sleep));
        }

        let result = self.sender.send(thread_id.clone(), content).await;

        match result {
            Ok(()) => {
                let reply = Reply {
                    thread_id: thread_id.clone(),
                    response_ms: start.elapsed().as_millis() as u64,
                    reason,
                    at_unix_ms: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
                };
                (self.notify)(BotEvent::Replied(reply));
                let bot = Arc::clone(&self);
                tokio::spawn(async move {
                    sleep(DEDUPE_TTL).await;
                    bot.lock().processed.remove(&thread_id);
                });
            }
            Err(e) => {
                let reverted = {
                    let mut g = self.lock();
                    g.processed.remove(&thread_id);
                    if sleeping && g.cfg.status == Status::Sleep {
                        g.cfg.status = Status::On;
                        true
                    } else {
                        false
                    }
                };
                (self.notify)(BotEvent::Info(format!("Erro ao enviar no tópico {thread_id}: {e}")));
                if reverted {
                    (self.notify)(BotEvent::StatusChanged(Status::On));
                }
            }
        }
    }

    // ── Controle (UI) ───────────────────────────────────────────────

    pub fn set_status(&self, status: Status) {
        let cancelled = {
            let mut g = self.lock();
            if g.cfg.status == status {
                return;
            }
            g.cfg.status = status;
            if status != Status::On { g.cancel_all() } else { 0 }
        };
        if cancelled > 0 {
            (self.notify)(BotEvent::Info(format!("{cancelled} tópico(s) pendente(s) cancelado(s)")));
        }
        (self.notify)(BotEvent::StatusChanged(status));
    }

    /// Aplica nova config ao vivo (o status atual é preservado).
    pub fn update_config(&self, mut next: Config) {
        let mut g = self.lock();
        next.status = g.cfg.status;
        if next.channel_id != g.cfg.channel_id {
            g.cancel_all();
        }
        g.cfg = next;
    }
}

// ─────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Mock {
        calls: Mutex<Vec<String>>,
        fail: bool,
    }
    impl Sender for Mock {
        fn send(&self, channel_id: String, _content: String) -> SendFuture {
            self.calls.lock().unwrap().push(channel_id);
            let fail = self.fail;
            Box::pin(async move { if fail { Err("HTTP 500".into()) } else { Ok(()) } })
        }
    }

    fn setup(mut cfg: Config, fail: bool) -> (Arc<Bot>, Arc<Mock>) {
        cfg.channel_id = "C1".into();
        cfg.auto_message = "oi".into();
        let mock = Arc::new(Mock { fail, ..Default::default() });
        let bot = Bot::new(cfg, mock.clone(), Arc::new(|_| {}));
        (bot, mock)
    }

    fn thread(id: &str) -> ThreadEvent {
        ThreadEvent {
            id: id.into(),
            parent_id: Some("C1".into()),
            owner_id: Some("U".into()),
            newly_created: true,
            name: Some("vaga".into()),
        }
    }

    async fn settle() {
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
    }

    fn calls(m: &Mock) -> Vec<String> {
        m.calls.lock().unwrap().clone()
    }

    #[tokio::test(start_paused = true)]
    async fn never_sends_on_thread_create_only_after_owner_message() {
        let (bot, mock) = setup(Config { humanized_enabled: false, ..Config::default() }, false);
        bot.on_thread_create(&ThreadEvent { parent_id: Some("OUTRO".into()), ..thread("X") });
        bot.on_thread_create(&ThreadEvent { newly_created: false, ..thread("Y") });
        bot.on_thread_create(&thread("Z"));
        settle().await;
        assert!(calls(&mock).is_empty());

        bot.on_message_create("Z", "OUTRO");
        settle().await;
        assert!(calls(&mock).is_empty());

        bot.on_message_create("Z", "U");
        settle().await;
        assert_eq!(calls(&mock), vec!["Z"]);
    }

    #[tokio::test(start_paused = true)]
    async fn humanized_delay_applies() {
        let (bot, mock) = setup(Config::default(), false); // 900ms
        bot.on_thread_create(&thread("T"));
        bot.on_message_create("T", "U");
        tokio::time::advance(Duration::from_millis(899)).await;
        settle().await;
        assert!(calls(&mock).is_empty());
        tokio::time::advance(Duration::from_millis(2)).await;
        settle().await;
        assert_eq!(calls(&mock), vec!["T"]);
        tokio::time::advance(Duration::from_secs(10)).await;
        settle().await;
        assert_eq!(calls(&mock).len(), 1, "timeout não dispara depois");
    }

    #[tokio::test(start_paused = true)]
    async fn early_owner_message() {
        let (bot, mock) = setup(Config { humanized_enabled: false, ..Config::default() }, false);
        bot.on_message_create("T", "U");
        bot.on_thread_create(&thread("T"));
        settle().await;
        assert_eq!(calls(&mock), vec!["T"]);
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_sends_without_owner_message() {
        let (bot, mock) = setup(Config::default(), false);
        bot.on_thread_create(&thread("T"));
        tokio::time::advance(Duration::from_millis(5001)).await;
        settle().await;
        assert_eq!(calls(&mock), vec!["T"]);
    }

    #[tokio::test(start_paused = true)]
    async fn off_cancels_pending_and_scheduled() {
        let (bot, mock) = setup(Config::default(), false);
        bot.on_thread_create(&thread("A"));
        bot.on_thread_create(&thread("B"));
        bot.on_message_create("B", "U"); // B agendado (900ms)
        bot.set_status(Status::Off);
        tokio::time::advance(Duration::from_secs(60)).await;
        settle().await;
        assert!(calls(&mock).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn channel_change_cancels_pending() {
        let (bot, mock) = setup(Config::default(), false);
        bot.on_thread_create(&thread("A"));
        let mut cfg = bot.config();
        cfg.channel_id = "C2".into();
        bot.update_config(cfg);
        tokio::time::advance(Duration::from_secs(60)).await;
        settle().await;
        assert!(calls(&mock).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn sleep_after_reply() {
        let cfg = Config { humanized_enabled: false, sleep_after_reply: true, ..Config::default() };
        let (bot, mock) = setup(cfg, false);
        bot.on_thread_create(&thread("A"));
        bot.on_thread_create(&thread("B"));
        bot.on_message_create("A", "U");
        settle().await;
        assert_eq!(bot.status(), Status::Sleep);

        tokio::time::advance(Duration::from_secs(60)).await; // B não envia (cancelado)
        bot.on_thread_create(&thread("C"));
        bot.on_message_create("C", "U");
        settle().await;
        assert_eq!(calls(&mock), vec!["A"]);

        bot.set_status(Status::On);
        bot.on_thread_create(&thread("D"));
        bot.on_message_create("D", "U");
        settle().await;
        assert_eq!(calls(&mock), vec!["A", "D"]);
    }

    #[tokio::test(start_paused = true)]
    async fn sleep_reverts_when_send_fails() {
        let cfg = Config { humanized_enabled: false, sleep_after_reply: true, ..Config::default() };
        let (bot, _mock) = setup(cfg, true);
        bot.on_thread_create(&thread("A"));
        bot.on_message_create("A", "U");
        settle().await;
        assert_eq!(bot.status(), Status::On);
    }

    #[tokio::test(start_paused = true)]
    async fn dedupe() {
        let (bot, mock) = setup(Config { humanized_enabled: false, ..Config::default() }, false);
        bot.on_thread_create(&thread("A"));
        bot.on_message_create("A", "U");
        settle().await;
        bot.on_thread_create(&thread("A"));
        bot.on_message_create("A", "U");
        settle().await;
        assert_eq!(calls(&mock), vec!["A"]);
    }
}
