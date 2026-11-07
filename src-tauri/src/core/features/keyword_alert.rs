//! Alerta de palavras (feature geral, independente do modo e do status):
//! quando uma mensagem no canal observado CONTÉM alguma das palavras/frases
//! configuradas, dispara som ou alarme.
//!
//! - Comparação com `text::normalize` dos dois lados (maiúsculas, acentos e
//!   espaços extras ignorados).
//! - Só mensagens do próprio canal (não dos tópicos dele).
//! - Mensagens da própria conta CONTAM (útil para testar), exceto a resposta
//!   automática do próprio bot (`ignore_text`).
//! - Cooldown entre disparos para não virar spam de som.

use crate::config::{AlertSound, KeywordAlertConfig};
use crate::core::text::normalize;
use tokio::time::{Duration, Instant};

pub const COOLDOWN: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Palavra como o usuário digitou.
    pub keyword: String,
    pub sound: AlertSound,
}

#[derive(Debug, Default)]
pub struct KeywordAlert {
    enabled: bool,
    sound: AlertSound,
    /// (texto original, texto normalizado)
    keywords: Vec<(String, String)>,
    last_fire: Option<Instant>,
}

pub struct Message<'a> {
    pub channel_id: &'a str,
    pub author_id: &'a str,
    pub content: &'a str,
}

impl KeywordAlert {
    pub fn from_config(cfg: &KeywordAlertConfig) -> Self {
        let mut a = Self::default();
        a.apply(cfg);
        a
    }

    /// Aplica nova configuração ao vivo (mantém o cooldown).
    pub fn apply(&mut self, cfg: &KeywordAlertConfig) {
        self.enabled = cfg.enabled;
        self.sound = cfg.sound;
        self.keywords = cfg
            .keywords
            .iter()
            .map(|k| (k.clone(), normalize(k)))
            .filter(|(_, n)| !n.is_empty())
            .collect();
    }

    pub fn is_active(&self) -> bool {
        self.enabled && !self.keywords.is_empty()
    }

    /// `own_reply`: (id da própria conta, texto da resposta automática) — essa
    /// mensagem específica, enviada pelo próprio bot, não dispara o alerta.
    pub fn check(&mut self, msg: &Message, watched_channel: &str, own_reply: Option<(&str, &str)>, now: Instant) -> Option<Hit> {
        if !self.is_active() || watched_channel.is_empty() || msg.channel_id != watched_channel || msg.content.is_empty() {
            return None;
        }
        let text = normalize(msg.content);
        if let Some((self_id, reply)) = own_reply {
            if msg.author_id == self_id && !reply.is_empty() && text == normalize(reply) {
                return None;
            }
        }
        if let Some(last) = self.last_fire {
            if now.saturating_duration_since(last) < COOLDOWN {
                return None;
            }
        }
        let (keyword, _) = self.keywords.iter().find(|(_, n)| text.contains(n.as_str()))?;
        self.last_fire = Some(now);
        Some(Hit { keyword: keyword.clone(), sound: self.sound })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alert(words: &[&str], sound: AlertSound) -> KeywordAlert {
        KeywordAlert::from_config(&KeywordAlertConfig {
            enabled: true,
            keywords: words.iter().map(|s| s.to_string()).collect(),
            sound,
        })
    }

    fn msg<'a>(channel: &'a str, author: &'a str, content: &'a str) -> Message<'a> {
        Message { channel_id: channel, author_id: author, content }
    }

    #[tokio::test(start_paused = true)]
    async fn matches_ignoring_case_accents_and_spaces() {
        let mut a = alert(&["PinG", "vaga  aberta"], AlertSound::Ding);
        let hit = a.check(&msg("C", "U", "alguém deu PÍNG?"), "C", None, Instant::now()).unwrap();
        assert_eq!(hit.keyword, "PinG");
        tokio::time::advance(COOLDOWN).await;
        let hit = a.check(&msg("C", "U", "VAGA ABERTA agora"), "C", None, Instant::now()).unwrap();
        assert_eq!(hit.keyword, "vaga  aberta");
    }

    #[tokio::test(start_paused = true)]
    async fn contains_inside_other_words() {
        let mut a = alert(&["ping"], AlertSound::Alarm);
        let hit = a.check(&msg("C", "U", "fui ao shopping"), "C", None, Instant::now()).unwrap();
        assert_eq!(hit.sound, AlertSound::Alarm);
    }

    #[tokio::test(start_paused = true)]
    async fn ignores_other_channels_and_threads() {
        let mut a = alert(&["ping"], AlertSound::Ding);
        let now = Instant::now();
        assert!(a.check(&msg("TOPICO", "U", "ping"), "C", None, now).is_none());
        assert!(a.check(&msg("C", "U", "nada aqui"), "C", None, now).is_none());
        assert!(a.check(&msg("C", "U", "ping"), "", None, now).is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn own_messages_count_except_the_bot_auto_reply() {
        let mut a = alert(&["ping"], AlertSound::Ding);
        let own = Some(("ME", "Pinguim aqui!"));
        // resposta automática do próprio bot: ignorada
        assert!(a.check(&msg("C", "ME", "pinguim AQUI!"), "C", own, Instant::now()).is_none());
        // qualquer outra mensagem da própria conta: dispara (permite testar sozinho)
        assert!(a.check(&msg("C", "ME", "teste PiNg"), "C", own, Instant::now()).is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn cooldown_between_hits() {
        let mut a = alert(&["ping"], AlertSound::Ding);
        assert!(a.check(&msg("C", "U", "ping"), "C", None, Instant::now()).is_some());
        assert!(a.check(&msg("C", "U", "ping"), "C", None, Instant::now()).is_none());
        tokio::time::advance(COOLDOWN).await;
        assert!(a.check(&msg("C", "U", "ping"), "C", None, Instant::now()).is_some());
    }

    #[tokio::test(start_paused = true)]
    async fn disabled_or_empty_never_fires() {
        let mut a = KeywordAlert::from_config(&KeywordAlertConfig {
            enabled: false,
            keywords: vec!["ping".into()],
            sound: AlertSound::Ding,
        });
        assert!(a.check(&msg("C", "U", "ping"), "C", None, Instant::now()).is_none());
        let mut b = alert(&["   "], AlertSound::Ding);
        assert!(!b.is_active());
        assert!(b.check(&msg("C", "U", "ping"), "C", None, Instant::now()).is_none());
    }
}
