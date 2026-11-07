//! Controle da regra "responder somente DEPOIS da 1ª mensagem do criador".
//!
//! Estrutura pura (sem timers, sem rede): guarda tópicos pendentes e um buffer
//! de mensagens "adiantadas" (a msg do criador pode chegar antes do
//! THREAD_CREATE). Os timers ficam no `Bot`.

use std::collections::HashMap;
use tokio::time::{Duration, Instant};

pub const BUFFER_TTL: Duration = Duration::from_secs(15);
pub const BUFFER_MAX: usize = 5000;

#[derive(Debug, Clone)]
pub struct Pending {
    pub owner_id: String,
    pub start: Instant,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ThreadDecision {
    /// O criador já postou no tópico: pode enviar.
    OwnerAlreadyPosted,
    /// Aguardar a 1ª msg do criador (com timeout).
    Wait,
}

#[derive(Debug)]
pub struct Humanized {
    pending: HashMap<String, Pending>,
    recent: HashMap<(String, String), Instant>,
    last_sweep: Instant,
}

impl Default for Humanized {
    fn default() -> Self {
        Self { pending: HashMap::new(), recent: HashMap::new(), last_sweep: Instant::now() }
    }
}

impl Humanized {
    pub fn on_thread_create(&mut self, thread_id: &str, owner_id: &str, start: Instant) -> ThreadDecision {
        let key = (thread_id.to_string(), owner_id.to_string());
        if let Some(seen) = self.recent.remove(&key) {
            if start.saturating_duration_since(seen) <= BUFFER_TTL {
                return ThreadDecision::OwnerAlreadyPosted;
            }
        }
        self.pending.insert(thread_id.to_string(), Pending { owner_id: owner_id.to_string(), start });
        ThreadDecision::Wait
    }

    /// Retorna `Some(start)` se a mensagem é a 1ª do criador de um tópico
    /// pendente (o tópico deixa de ser pendente). Caso contrário, guarda no
    /// buffer para o caso do THREAD_CREATE chegar depois.
    pub fn on_message_create(&mut self, channel_id: &str, author_id: &str, now: Instant) -> Option<Instant> {
        if let Some(p) = self.pending.get(channel_id) {
            if p.owner_id != author_id {
                return None;
            }
            return self.pending.remove(channel_id).map(|p| p.start);
        }
        if self.recent.len() >= BUFFER_MAX || now.saturating_duration_since(self.last_sweep) > BUFFER_TTL {
            self.sweep(now);
        }
        if self.recent.len() < BUFFER_MAX {
            self.recent.insert((channel_id.to_string(), author_id.to_string()), now);
        }
        None
    }

    pub fn take_pending(&mut self, thread_id: &str) -> Option<Pending> {
        self.pending.remove(thread_id)
    }

    pub fn is_pending(&self, thread_id: &str) -> bool {
        self.pending.contains_key(thread_id)
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.recent.clear();
    }

    fn sweep(&mut self, now: Instant) {
        self.recent.retain(|_, ts| now.saturating_duration_since(*ts) <= BUFFER_TTL);
        self.last_sweep = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn owner_message_resolves_pending() {
        let mut h = Humanized::default();
        let start = Instant::now();
        assert_eq!(h.on_thread_create("T", "U", start), ThreadDecision::Wait);
        assert_eq!(h.on_message_create("T", "OUTRO", Instant::now()), None);
        assert_eq!(h.on_message_create("T", "U", Instant::now()), Some(start));
        assert!(!h.is_pending("T"));
    }

    #[tokio::test(start_paused = true)]
    async fn early_message_before_thread_create() {
        let mut h = Humanized::default();
        h.on_message_create("T", "U", Instant::now());
        assert_eq!(h.on_thread_create("T", "U", Instant::now()), ThreadDecision::OwnerAlreadyPosted);
    }

    #[tokio::test(start_paused = true)]
    async fn early_message_expires() {
        let mut h = Humanized::default();
        h.on_message_create("T", "U", Instant::now());
        tokio::time::advance(Duration::from_secs(16)).await;
        assert_eq!(h.on_thread_create("T", "U", Instant::now()), ThreadDecision::Wait);
    }
}
