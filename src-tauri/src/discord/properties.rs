//! Identidade do cliente (fingerprint) usada no IDENTIFY do gateway e nos
//! headers REST (User-Agent / X-Super-Properties). Manter consistentes.

use base64::Engine as _;
use serde_json::{json, Value};
use std::sync::OnceLock;

pub const API_VERSION: u8 = 9;
pub const BROWSER_VERSION: &str = "140.0.0.0";
pub const LOCALE: &str = "pt-BR";
pub const CAPABILITIES: u32 = 30717;
const CLIENT_BUILD_NUMBER: u64 = 440000;

pub fn user_agent() -> &'static str {
    static UA: OnceLock<String> = OnceLock::new();
    UA.get_or_init(|| {
        format!(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{BROWSER_VERSION} Safari/537.36"
        )
    })
}

pub fn properties() -> &'static Value {
    static P: OnceLock<Value> = OnceLock::new();
    P.get_or_init(|| {
        json!({
            "os": "Windows",
            "browser": "Chrome",
            "device": "",
            "system_locale": LOCALE,
            "has_client_mods": false,
            "browser_user_agent": user_agent(),
            "browser_version": BROWSER_VERSION,
            "os_version": "10",
            "referrer": "",
            "referring_domain": "",
            "referrer_current": "",
            "referring_domain_current": "",
            "release_channel": "stable",
            "client_build_number": CLIENT_BUILD_NUMBER,
            "client_event_source": null
        })
    })
}

pub fn super_properties() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| base64::engine::general_purpose::STANDARD.encode(properties().to_string()))
}
