//! Token do Discord no Cofre do Windows (Credential Manager), criptografado
//! pela conta do Windows. Nunca gravar em arquivo/log.

const SERVICE: &str = "AutoPing";
const USER: &str = "discord-token";

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, USER).map_err(|e| format!("Cofre indisponível: {e}"))
}

pub fn load_token() -> Option<String> {
    entry().ok()?.get_password().ok().filter(|t| !t.trim().is_empty())
}

pub fn save_token(token: &str) -> Result<(), String> {
    entry()?.set_password(token).map_err(|e| format!("Falha ao salvar token: {e}"))
}

pub fn delete_token() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Falha ao remover token: {e}")),
    }
}
