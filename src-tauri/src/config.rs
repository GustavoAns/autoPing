//! Configuração persistida em JSON (%APPDATA%/AutoPing/config.json).
//! O token NÃO fica aqui: vai para o Cofre do Windows (ver `secrets.rs`).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_WAIT_MS: u64 = 5000;
pub const MIN_WAIT_MS: u64 = 500;
pub const DEFAULT_HUMANIZED_DELAY_MS: u64 = 900;
pub const MAX_MESSAGE_LEN: usize = 2000;

/// Tipos de canal que podem ter tópicos: texto, anúncios, fórum, mídia.
pub const THREAD_PARENT_TYPES: [u8; 4] = [0, 5, 15, 16];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Ligado: responde tópicos.
    On,
    /// Desligado manualmente.
    Off,
    /// Desligado automaticamente após responder (sleep após responder).
    Sleep,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub channel_id: String,
    pub auto_message: String,
    /// Tempo máx. aguardando a 1ª msg do criador antes de enviar mesmo assim.
    pub wait_for_message_ms: u64,
    /// Modo humano: aplica `humanized_delay_ms` após a 1ª msg do criador.
    pub humanized_enabled: bool,
    pub humanized_delay_ms: u64,
    /// Após responder, entra em Sleep até ser ligado de novo.
    pub sleep_after_reply: bool,
    /// Toca um aviso curto quando a mensagem é enviada.
    pub sound_enabled: bool,
    /// Modo alarme: após enviar, toca um alarme contínuo até ser parado.
    pub alarm_enabled: bool,
    pub status: Status,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            channel_id: String::new(),
            auto_message: String::new(),
            wait_for_message_ms: DEFAULT_WAIT_MS,
            humanized_enabled: true,
            humanized_delay_ms: DEFAULT_HUMANIZED_DELAY_MS,
            sleep_after_reply: false,
            sound_enabled: true,
            alarm_enabled: false,
            status: Status::On,
        }
    }
}

/// Alteração parcial vinda da interface.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConfigPatch {
    pub channel_id: Option<String>,
    pub auto_message: Option<String>,
    pub wait_for_message_ms: Option<u64>,
    pub humanized_enabled: Option<bool>,
    pub humanized_delay_ms: Option<u64>,
    pub sleep_after_reply: Option<bool>,
    pub sound_enabled: Option<bool>,
    pub alarm_enabled: Option<bool>,
}

impl Config {
    /// Delay efetivo após a 1ª msg do criador.
    pub fn effective_delay_ms(&self) -> u64 {
        if self.humanized_enabled {
            self.humanized_delay_ms
        } else {
            0
        }
    }

    pub fn load(path: &Path) -> Self {
        let mut cfg: Config = fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if cfg.wait_for_message_ms < MIN_WAIT_MS {
            cfg.wait_for_message_ms = DEFAULT_WAIT_MS;
        }
        cfg
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
        if let Some(msg) = &patch.auto_message {
            validate_message(msg)?;
            next.auto_message = msg.clone();
        }
        if let Some(ms) = patch.wait_for_message_ms {
            if ms < MIN_WAIT_MS {
                return Err(format!("Tempo de espera mínimo: {MIN_WAIT_MS}ms."));
            }
            next.wait_for_message_ms = ms;
        }
        if let Some(v) = patch.humanized_enabled {
            next.humanized_enabled = v;
        }
        if let Some(ms) = patch.humanized_delay_ms {
            if ms > 60_000 {
                return Err("Delay humano máximo: 60000ms.".into());
            }
            next.humanized_delay_ms = ms;
        }
        if let Some(v) = patch.sleep_after_reply {
            next.sleep_after_reply = v;
        }
        if let Some(v) = patch.sound_enabled {
            next.sound_enabled = v;
        }
        if let Some(v) = patch.alarm_enabled {
            next.alarm_enabled = v;
        }
        Ok(next)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let c = Config::default();
        assert_eq!(c.wait_for_message_ms, 5000);
        assert_eq!(c.effective_delay_ms(), 900);
        assert_eq!(c.status, Status::On);
        assert!(!c.sleep_after_reply);
        assert!(c.sound_enabled);
        assert!(!c.alarm_enabled);
    }

    #[test]
    fn humanized_off_means_zero_delay() {
        let c = Config { humanized_enabled: false, ..Config::default() };
        assert_eq!(c.effective_delay_ms(), 0);
    }

    #[test]
    fn patch_validation() {
        let c = Config::default();
        assert!(c.with_patch(&ConfigPatch { channel_id: Some("abc".into()), ..Default::default() }).is_err());
        assert!(c.with_patch(&ConfigPatch { wait_for_message_ms: Some(100), ..Default::default() }).is_err());
        assert!(c.with_patch(&ConfigPatch { auto_message: Some("  ".into()), ..Default::default() }).is_err());
        let n = c
            .with_patch(&ConfigPatch {
                channel_id: Some(" 745077089866219626 ".into()),
                auto_message: Some("oi".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(n.channel_id, "745077089866219626");
        assert_eq!(n.auto_message, "oi");
    }

    #[test]
    fn json_roundtrip_and_partial_file() {
        let c: Config = serde_json::from_str(r#"{"channelId":"1","status":"sleep"}"#).unwrap();
        assert_eq!(c.status, Status::Sleep);
        assert_eq!(c.wait_for_message_ms, 5000);
    }
}
