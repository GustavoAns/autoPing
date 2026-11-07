//! Configuração persistida em JSON (%APPDATA%/com.autoping.desktop/config.json).
//! O token NÃO fica aqui: vai para o Cofre do Windows (ver `secrets.rs`).
//!
//! Estrutura:
//! - geral: canal observado, status, sleep e som/alarme ao responder
//! - `mode` + `modes.<modo>`: modo de ping ativo e as configurações de cada modo
//! - `keywordAlert`: feature geral de alerta de palavras

use crate::core::text::normalize;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_WAIT_MS: u64 = 5000;
pub const MIN_WAIT_MS: u64 = 500;
pub const DEFAULT_HUMANIZED_DELAY_MS: u64 = 900;
pub const MAX_MESSAGE_LEN: usize = 2000;
pub const MAX_KEYWORDS: usize = 50;
pub const MAX_KEYWORD_LEN: usize = 100;

/// Tipos de canal que podem ter tópicos: texto, anúncios, fórum, mídia.
pub const THREAD_PARENT_TYPES: [u8; 4] = [0, 5, 15, 16];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Ligado: o modo ativo responde.
    On,
    /// Desligado manualmente.
    Off,
    /// Desligado automaticamente após responder (sleep após responder).
    Sleep,
}

/// Modos de ping disponíveis. Novos modos entram aqui e em `core/modes`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModeKind {
    /// Responde novos tópicos do canal observado.
    #[default]
    Thread,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertSound {
    #[default]
    Ding,
    Alarm,
}

/// Configurações do modo "Tópico".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ThreadSettings {
    pub auto_message: String,
    /// Tempo máx. aguardando a 1ª msg do criador antes de enviar mesmo assim.
    pub wait_for_message_ms: u64,
    /// Modo humano: aplica `humanized_delay_ms` após a msg do criador.
    pub humanized_enabled: bool,
    pub humanized_delay_ms: u64,
}

impl Default for ThreadSettings {
    fn default() -> Self {
        Self {
            auto_message: String::new(),
            wait_for_message_ms: DEFAULT_WAIT_MS,
            humanized_enabled: true,
            humanized_delay_ms: DEFAULT_HUMANIZED_DELAY_MS,
        }
    }
}

impl ThreadSettings {
    /// Delay efetivo após a 1ª msg do criador.
    pub fn effective_delay_ms(&self) -> u64 {
        if self.humanized_enabled {
            self.humanized_delay_ms
        } else {
            0
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ModesConfig {
    pub thread: ThreadSettings,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct KeywordAlertConfig {
    pub enabled: bool,
    pub keywords: Vec<String>,
    pub sound: AlertSound,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    /// Canal observado (usado pelo modo ativo e pelas features gerais).
    pub channel_id: String,
    pub status: Status,
    /// Após responder, entra em Sleep até ser ligado de novo.
    pub sleep_after_reply: bool,
    /// Aviso curto quando a mensagem é enviada.
    pub sound_on_reply: bool,
    /// Alarme contínuo após responder, até o usuário parar.
    pub alarm_on_reply: bool,
    pub mode: ModeKind,
    pub modes: ModesConfig,
    pub keyword_alert: KeywordAlertConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            channel_id: String::new(),
            status: Status::On,
            sleep_after_reply: false,
            sound_on_reply: true,
            alarm_on_reply: false,
            mode: ModeKind::Thread,
            modes: ModesConfig::default(),
            keyword_alert: KeywordAlertConfig::default(),
        }
    }
}

// ── Patch vindo da interface ────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ThreadPatch {
    pub auto_message: Option<String>,
    pub wait_for_message_ms: Option<u64>,
    pub humanized_enabled: Option<bool>,
    pub humanized_delay_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct KeywordAlertPatch {
    pub enabled: Option<bool>,
    pub keywords: Option<Vec<String>>,
    pub sound: Option<AlertSound>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConfigPatch {
    pub channel_id: Option<String>,
    pub sleep_after_reply: Option<bool>,
    pub sound_on_reply: Option<bool>,
    pub alarm_on_reply: Option<bool>,
    pub mode: Option<ModeKind>,
    pub thread: Option<ThreadPatch>,
    pub keyword_alert: Option<KeywordAlertPatch>,
}

impl Config {
    /// Carrega o arquivo. Retorna `(config, migrou)`; se migrou do formato
    /// antigo (v3.0), o chamador deve regravar.
    pub fn load(path: &Path) -> (Self, bool) {
        let Some(value) = fs::read_to_string(path).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) else {
            return (Config::default(), false);
        };
        let (mut cfg, migrated) = if is_legacy(&value) {
            (from_legacy(&value), true)
        } else {
            (serde_json::from_value(value).unwrap_or_default(), false)
        };
        if cfg.modes.thread.wait_for_message_ms < MIN_WAIT_MS {
            cfg.modes.thread.wait_for_message_ms = DEFAULT_WAIT_MS;
        }
        (cfg, migrated)
    }

    /// Escrita atômica (arquivo temporário + rename).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp: PathBuf = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self).unwrap_or_default())?;
        fs::rename(tmp, path)
    }

    /// Valida e aplica o patch, retornando a nova config (sem alterar `self`).
    pub fn with_patch(&self, patch: &ConfigPatch) -> Result<Config, String> {
        let mut next = self.clone();
        if let Some(id) = &patch.channel_id {
            let id = id.trim();
            if !id.is_empty() && !is_snowflake(id) {
                return Err("ID do canal inválido (17-20 dígitos).".into());
            }
            next.channel_id = id.to_string();
        }
        if let Some(v) = patch.sleep_after_reply {
            next.sleep_after_reply = v;
        }
        if let Some(v) = patch.sound_on_reply {
            next.sound_on_reply = v;
        }
        if let Some(v) = patch.alarm_on_reply {
            next.alarm_on_reply = v;
        }
        if let Some(m) = patch.mode {
            next.mode = m;
        }
        if let Some(t) = &patch.thread {
            let s = &mut next.modes.thread;
            if let Some(msg) = &t.auto_message {
                validate_message(msg)?;
                s.auto_message = msg.clone();
            }
            if let Some(ms) = t.wait_for_message_ms {
                if ms < MIN_WAIT_MS {
                    return Err(format!("Tempo de espera mínimo: {MIN_WAIT_MS}ms."));
                }
                s.wait_for_message_ms = ms;
            }
            if let Some(v) = t.humanized_enabled {
                s.humanized_enabled = v;
            }
            if let Some(ms) = t.humanized_delay_ms {
                if ms > 60_000 {
                    return Err("Delay humano máximo: 60000ms.".into());
                }
                s.humanized_delay_ms = ms;
            }
        }
        if let Some(k) = &patch.keyword_alert {
            let a = &mut next.keyword_alert;
            if let Some(words) = &k.keywords {
                a.keywords = clean_keywords(words)?;
            }
            if let Some(v) = k.sound {
                a.sound = v;
            }
            if let Some(v) = k.enabled {
                a.enabled = v;
            }
            if a.enabled && a.keywords.is_empty() {
                return Err("Adicione ao menos uma palavra para o alerta de palavras.".into());
            }
        }
        Ok(next)
    }
}

/// Trim, descarta vazias, remove duplicadas (após normalizar), limita tamanho.
pub fn clean_keywords(words: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for w in words {
        let w = w.trim();
        if w.is_empty() {
            continue;
        }
        if w.chars().count() > MAX_KEYWORD_LEN {
            return Err(format!("Cada palavra pode ter até {MAX_KEYWORD_LEN} caracteres."));
        }
        let n = normalize(w);
        if n.is_empty() || !seen.insert(n) {
            continue;
        }
        out.push(w.to_string());
    }
    if out.len() > MAX_KEYWORDS {
        return Err(format!("Máximo de {MAX_KEYWORDS} palavras."));
    }
    Ok(out)
}

pub fn validate_message(msg: &str) -> Result<(), String> {
    if msg.trim().is_empty() {
        return Err("A mensagem não pode estar vazia.".into());
    }
    if msg.chars().count() > MAX_MESSAGE_LEN {
        return Err(format!("A mensagem não pode ter mais de {MAX_MESSAGE_LEN} caracteres."));
    }
    Ok(())
}

pub fn is_snowflake(id: &str) -> bool {
    (17..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

// ── Migração do formato v3.0 (campos planos) ────────────────────────

fn is_legacy(v: &Value) -> bool {
    ["autoMessage", "waitForMessageMs", "humanizedEnabled", "humanizedDelayMs", "soundEnabled", "alarmEnabled"]
        .iter()
        .any(|k| v.get(k).is_some())
}

fn from_legacy(v: &Value) -> Config {
    let d = Config::default();
    let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let b = |k: &str, def: bool| v.get(k).and_then(Value::as_bool).unwrap_or(def);
    let n = |k: &str, def: u64| v.get(k).and_then(Value::as_u64).unwrap_or(def);
    let status = v.get("status").and_then(|x| serde_json::from_value(x.clone()).ok()).unwrap_or(Status::On);
    Config {
        channel_id: s("channelId").unwrap_or_default(),
        status,
        sleep_after_reply: b("sleepAfterReply", d.sleep_after_reply),
        sound_on_reply: b("soundEnabled", d.sound_on_reply),
        alarm_on_reply: b("alarmEnabled", d.alarm_on_reply),
        mode: ModeKind::Thread,
        modes: ModesConfig {
            thread: ThreadSettings {
                auto_message: s("autoMessage").unwrap_or_default(),
                wait_for_message_ms: n("waitForMessageMs", DEFAULT_WAIT_MS),
                humanized_enabled: b("humanizedEnabled", true),
                humanized_delay_ms: n("humanizedDelayMs", DEFAULT_HUMANIZED_DELAY_MS),
            },
        },
        keyword_alert: KeywordAlertConfig::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let c = Config::default();
        assert_eq!(c.mode, ModeKind::Thread);
        assert_eq!(c.modes.thread.wait_for_message_ms, 5000);
        assert_eq!(c.modes.thread.effective_delay_ms(), 900);
        assert_eq!(c.status, Status::On);
        assert!(c.sound_on_reply && !c.alarm_on_reply && !c.sleep_after_reply);
        assert!(!c.keyword_alert.enabled);
    }

    #[test]
    fn humanized_off_means_zero_delay() {
        let t = ThreadSettings { humanized_enabled: false, ..ThreadSettings::default() };
        assert_eq!(t.effective_delay_ms(), 0);
    }

    #[test]
    fn patch_validation() {
        let c = Config::default();
        assert!(c.with_patch(&ConfigPatch { channel_id: Some("abc".into()), ..Default::default() }).is_err());
        let wait = ConfigPatch { thread: Some(ThreadPatch { wait_for_message_ms: Some(100), ..Default::default() }), ..Default::default() };
        assert!(c.with_patch(&wait).is_err());
        let empty = ConfigPatch { thread: Some(ThreadPatch { auto_message: Some("  ".into()), ..Default::default() }), ..Default::default() };
        assert!(c.with_patch(&empty).is_err());
        let ok = ConfigPatch {
            channel_id: Some(" 745077089866219626 ".into()),
            thread: Some(ThreadPatch { auto_message: Some("oi".into()), ..Default::default() }),
            ..Default::default()
        };
        let n = c.with_patch(&ok).unwrap();
        assert_eq!(n.channel_id, "745077089866219626");
        assert_eq!(n.modes.thread.auto_message, "oi");
    }

    #[test]
    fn keywords_are_cleaned() {
        let words: Vec<String> = ["  ping ", "PING", "Píng", "", "vaga aberta"].iter().map(|s| s.to_string()).collect();
        assert_eq!(clean_keywords(&words).unwrap(), vec!["ping", "vaga aberta"]);
        let c = Config::default();
        let p = ConfigPatch {
            keyword_alert: Some(KeywordAlertPatch { enabled: Some(true), keywords: Some(vec![]), sound: None }),
            ..Default::default()
        };
        assert!(c.with_patch(&p).is_err(), "ligado sem palavras");
    }

    #[test]
    fn migrates_v30_flat_config() {
        let dir = std::env::temp_dir().join(format!("autoping-test-{}", std::process::id()));
        let path = dir.join("config.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            &path,
            r#"{"channelId":"745077089866219626","autoMessage":"teste","waitForMessageMs":6000,
                "humanizedEnabled":true,"humanizedDelayMs":450,"sleepAfterReply":true,
                "soundEnabled":false,"alarmEnabled":true,"status":"sleep"}"#,
        )
        .unwrap();
        let (c, migrated) = Config::load(&path);
        assert!(migrated);
        assert_eq!(c.channel_id, "745077089866219626");
        assert_eq!(c.status, Status::Sleep);
        assert!(c.sleep_after_reply && !c.sound_on_reply && c.alarm_on_reply);
        assert_eq!(c.modes.thread.auto_message, "teste");
        assert_eq!(c.modes.thread.wait_for_message_ms, 6000);
        assert_eq!(c.modes.thread.humanized_delay_ms, 450);

        c.save(&path).unwrap();
        let (again, migrated) = Config::load(&path);
        assert!(!migrated);
        assert_eq!(again, c);
        let _ = fs::remove_dir_all(dir);
    }
}
