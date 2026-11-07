//! Modo "Tópico": responde novos tópicos do canal observado.
//!
//! Regras:
//! - Nunca envia no THREAD_CREATE. O gatilho é a 1ª msg do criador dentro do
//!   tópico (o Discord cria o tópico e só depois posta a msg inicial).
//! - Sem msg do criador em `wait_for_message_ms` -> envia mesmo assim.
//! - Modo humano: espera `humanized_delay_ms` após a msg do criador.

pub mod humanized;

use super::{MessageEvent, Mode, ThreadEvent};
use crate::config::{Config, ModeKind, ThreadSettings};
use crate::core::reply::{BotEvent, ReplyGate};
use humanized::{Humanized, ThreadDecision};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::task::AbortHandle;
use tokio::time::{sleep_until, Duration, Instant};

struct Inner {
    channel_id: String,
    settings: ThreadSettings,
    waiter: Humanized,
    timers: HashMap<String, AbortHandle>,
}

pub struct ThreadMode {
    inner: Mutex<Inner>,
    gate: Arc<ReplyGate>,
}

impl ThreadMode {
    pub fn new(cfg: &Config, gate: Arc<ReplyGate>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                channel_id: cfg.channel_id.clone(),
                settings: cfg.modes.thread.clone(),
                waiter: Humanized::default(),
                timers: HashMap::new(),
            }),
            gate,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn schedule_send(self: &Arc<Self>, g: &mut Inner, thread_id: String, start: Instant, reason: &'static str) {
        let delay = g.settings.effective_delay_ms();
        let mode = Arc::clone(self);
        if delay == 0 {
            tokio::spawn(async move { mode.send(thread_id, start, reason).await });
            return;
        }
        let id = thread_id.clone();
        let deadline = Instant::now() + Duration::from_millis(delay);
        let h = tokio::spawn(async move {
            sleep_until(deadline).await;
            mode.send(id, start, reason).await;
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
            self.gate.notify(BotEvent::Info(format!("Timeout em {thread_id} - enviando mesmo assim")));
            self.send(thread_id, start, "timeout").await;
        }
    }

    async fn send(self: Arc<Self>, thread_id: String, start: Instant, reason: &'static str) {
        let content = {
            let mut g = self.lock();
            g.timers.remove(&thread_id); // este timer já disparou
            g.settings.auto_message.clone()
        };
        self.gate.clone().send(thread_id, content, start, reason).await;
    }
}

impl Mode for ThreadMode {
    fn kind(&self) -> ModeKind {
        ModeKind::Thread
    }

    fn on_thread_create(self: Arc<Self>, ev: &ThreadEvent) {
        let start = Instant::now();
        let mut g = self.lock();
        if g.settings.auto_message.is_empty() {
            return;
        }
        if !ev.newly_created || ev.parent_id.as_deref() != Some(g.channel_id.as_str()) {
            return;
        }
        if g.timers.contains_key(&ev.id) || g.waiter.is_pending(&ev.id) || self.gate.was_replied(&ev.id) {
            return;
        }
        let owner = ev.owner_id.clone().unwrap_or_default();
        let waiting = match g.waiter.on_thread_create(&ev.id, &owner, start) {
            ThreadDecision::OwnerAlreadyPosted => {
                self.schedule_send(&mut g, ev.id.clone(), start, "owner-message-early");
                false
            }
            ThreadDecision::Wait => {
                let deadline = start + Duration::from_millis(g.settings.wait_for_message_ms);
                let mode = Arc::clone(&self);
                let id = ev.id.clone();
                let h = tokio::spawn(async move {
                    sleep_until(deadline).await;
                    mode.on_timeout(id).await;
                });
                g.timers.insert(ev.id.clone(), h.abort_handle());
                true
            }
        };
        let wait_ms = g.settings.wait_for_message_ms;
        drop(g);
        let name = ev.name.as_deref().unwrap_or("?");
        self.gate.notify(BotEvent::Info(if waiting {
            format!("Novo tópico \"{name}\" ({}) - aguardando 1ª msg do criador (até {wait_ms}ms)", ev.id)
        } else {
            format!("Novo tópico \"{name}\" ({}) - criador já postou", ev.id)
        }));
    }

    fn on_message_create(self: Arc<Self>, msg: &MessageEvent) {
        let mut g = self.lock();
        if let Some(start) = g.waiter.on_message_create(msg.channel_id, msg.author_id, Instant::now()) {
            if let Some(h) = g.timers.remove(msg.channel_id) {
                h.abort(); // cancela o timeout
            }
            self.schedule_send(&mut g, msg.channel_id.to_string(), start, "owner-message");
        }
    }

    fn apply(&self, cfg: &Config) {
        let mut g = self.lock();
        if cfg.channel_id != g.channel_id {
            for (_, h) in g.timers.drain() {
                h.abort();
            }
            g.waiter.clear();
            g.channel_id = cfg.channel_id.clone();
        }
        g.settings = cfg.modes.thread.clone();
    }

    fn cancel_all(&self) -> usize {
        let mut g = self.lock();
        let n = g.waiter.pending_count().max(g.timers.len());
        for (_, h) in g.timers.drain() {
            h.abort();
        }
        g.waiter.clear();
        n
    }
}

// ─────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use crate::config::{Config, Status, ThreadSettings};
    use crate::core::modes::{MessageEvent, ModeRunner, ThreadEvent};
    use crate::core::reply::{SendFuture, Sender};
    use std::sync::{Arc, Mutex};
    use tokio::time::Duration;

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

    fn cfg(humanized: bool, sleep_after_reply: bool) -> Config {
        let mut c = Config::default();
        c.channel_id = "C1".into();
        c.sleep_after_reply = sleep_after_reply;
        c.modes.thread = ThreadSettings { auto_message: "oi".into(), humanized_enabled: humanized, ..ThreadSettings::default() };
        c
    }

    fn setup(c: Config, fail: bool) -> (Arc<ModeRunner>, Arc<Mock>) {
        let mock = Arc::new(Mock { fail, ..Default::default() });
        (ModeRunner::new(&c, mock.clone(), Arc::new(|_| {})), mock)
    }

    fn thread(id: &str) -> ThreadEvent {
        ThreadEvent { id: id.into(), parent_id: Some("C1".into()), owner_id: Some("U".into()), newly_created: true, name: Some("vaga".into()) }
    }

    fn msg<'a>(channel: &'a str, author: &'a str) -> MessageEvent<'a> {
        MessageEvent { channel_id: channel, author_id: author, content: "" }
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
        let (r, mock) = setup(cfg(false, false), false);
        r.on_thread_create(&ThreadEvent { parent_id: Some("OUTRO".into()), ..thread("X") });
        r.on_thread_create(&ThreadEvent { newly_created: false, ..thread("Y") });
        r.on_thread_create(&thread("Z"));
        settle().await;
        assert!(calls(&mock).is_empty());
        r.on_message_create(&msg("Z", "OUTRO"));
        settle().await;
        assert!(calls(&mock).is_empty());
        r.on_message_create(&msg("Z", "U"));
        settle().await;
        assert_eq!(calls(&mock), vec!["Z"]);
    }

    #[tokio::test(start_paused = true)]
    async fn humanized_delay_applies() {
        let (r, mock) = setup(cfg(true, false), false); // 900ms
        r.on_thread_create(&thread("T"));
        r.on_message_create(&msg("T", "U"));
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
        let (r, mock) = setup(cfg(false, false), false);
        r.on_message_create(&msg("T", "U"));
        r.on_thread_create(&thread("T"));
        settle().await;
        assert_eq!(calls(&mock), vec!["T"]);
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_sends_without_owner_message() {
        let (r, mock) = setup(cfg(true, false), false);
        r.on_thread_create(&thread("T"));
        tokio::time::advance(Duration::from_millis(5001)).await;
        settle().await;
        assert_eq!(calls(&mock), vec!["T"]);
    }

    #[tokio::test(start_paused = true)]
    async fn off_cancels_pending_and_scheduled() {
        let (r, mock) = setup(cfg(true, false), false);
        r.on_thread_create(&thread("A"));
        r.on_thread_create(&thread("B"));
        r.on_message_create(&msg("B", "U")); // B agendado (900ms)
        r.set_status(Status::Off);
        tokio::time::advance(Duration::from_secs(60)).await;
        settle().await;
        assert!(calls(&mock).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn channel_change_cancels_pending() {
        let (r, mock) = setup(cfg(true, false), false);
        r.on_thread_create(&thread("A"));
        let mut c = cfg(true, false);
        c.channel_id = "C2".into();
        r.apply(&c);
        tokio::time::advance(Duration::from_secs(60)).await;
        settle().await;
        assert!(calls(&mock).is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn settings_apply_live() {
        let (r, mock) = setup(cfg(true, false), false);
        r.apply(&cfg(false, false)); // desliga modo humano ao vivo
        r.on_thread_create(&thread("A"));
        r.on_message_create(&msg("A", "U"));
        settle().await;
        assert_eq!(calls(&mock), vec!["A"]);
    }

    #[tokio::test(start_paused = true)]
    async fn sleep_after_reply() {
        let (r, mock) = setup(cfg(false, true), false);
        r.on_thread_create(&thread("A"));
        r.on_thread_create(&thread("B"));
        r.on_message_create(&msg("A", "U"));
        settle().await;
        assert_eq!(r.status(), Status::Sleep);

        tokio::time::advance(Duration::from_secs(60)).await; // B cancelado
        r.on_thread_create(&thread("C"));
        r.on_message_create(&msg("C", "U"));
        settle().await;
        assert_eq!(calls(&mock), vec!["A"]);

        r.set_status(Status::On);
        r.on_thread_create(&thread("D"));
        r.on_message_create(&msg("D", "U"));
        settle().await;
        assert_eq!(calls(&mock), vec!["A", "D"]);
    }

    #[tokio::test(start_paused = true)]
    async fn sleep_reverts_when_send_fails() {
        let (r, _mock) = setup(cfg(false, true), true);
        r.on_thread_create(&thread("A"));
        r.on_message_create(&msg("A", "U"));
        settle().await;
        assert_eq!(r.status(), Status::On);
    }

    #[tokio::test(start_paused = true)]
    async fn dedupe() {
        let (r, mock) = setup(cfg(false, false), false);
        r.on_thread_create(&thread("A"));
        r.on_message_create(&msg("A", "U"));
        settle().await;
        r.on_thread_create(&thread("A"));
        r.on_message_create(&msg("A", "U"));
        settle().await;
        assert_eq!(calls(&mock), vec!["A"]);
    }
}
