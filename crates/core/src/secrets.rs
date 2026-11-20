//! Secrets in the OS store (Windows Credential Manager / macOS Keychain),
//! never in `settings.json`, the database or logs (PRD §7).

const SERVICE: &str = "Studio Lokalsi";
const HF_TOKEN_USER: &str = "huggingface-token";

#[derive(Debug, thiserror::Error)]
#[error("system credential store: {0}")]
pub struct SecretError(String);

fn entry(user: &str) -> Result<keyring::Entry, SecretError> {
    keyring::Entry::new(SERVICE, user).map_err(|e| SecretError(e.to_string()))
}

/// The stored Hugging Face token, if any.
pub fn hf_token() -> Result<Option<String>, SecretError> {
    match entry(HF_TOKEN_USER)?.get_password() {
        Ok(t) if !t.trim().is_empty() => Ok(Some(t)),
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(SecretError(e.to_string())),
    }
}

pub fn set_hf_token(token: &str) -> Result<(), SecretError> {
    let token = token.trim();
    if token.is_empty() {
        return clear_hf_token();
    }
    entry(HF_TOKEN_USER)?
        .set_password(token)
        .map_err(|e| SecretError(e.to_string()))?;
    tracing::info!("Hugging Face token saved in the system credential store");
    Ok(())
}

pub fn clear_hf_token() -> Result<(), SecretError> {
    match entry(HF_TOKEN_USER)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            tracing::info!("Hugging Face token removed");
            Ok(())
        }
        Err(e) => Err(SecretError(e.to_string())),
    }
}

fn device_user(device_id: &str) -> String {
    format!("device:{device_id}")
}

/// Access key of a phone paired with the local API (PRD §15 item 12).
pub fn device_key(device_id: &str) -> Result<Option<String>, SecretError> {
    match entry(&device_user(device_id))?.get_password() {
        Ok(k) if !k.is_empty() => Ok(Some(k)),
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(SecretError(e.to_string())),
    }
}

pub fn set_device_key(device_id: &str, key: &str) -> Result<(), SecretError> {
    entry(&device_user(device_id))?
        .set_password(key)
        .map_err(|e| SecretError(e.to_string()))
}

pub fn delete_device_key(device_id: &str) -> Result<(), SecretError> {
    match entry(&device_user(device_id))?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(SecretError(e.to_string())),
    }
}

fn api_key_user(key_id: &str) -> String {
    format!("api-key:{key_id}")
}

/// Whole access key (`lk-<id>.<secret>`) of the local server (ADR 0025).
pub fn api_key(key_id: &str) -> Result<Option<String>, SecretError> {
    match entry(&api_key_user(key_id))?.get_password() {
        Ok(k) if !k.is_empty() => Ok(Some(k)),
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(SecretError(e.to_string())),
    }
}

pub fn set_api_key(key_id: &str, token: &str) -> Result<(), SecretError> {
    entry(&api_key_user(key_id))?
        .set_password(token)
        .map_err(|e| SecretError(e.to_string()))
}

pub fn delete_api_key(key_id: &str) -> Result<(), SecretError> {
    match entry(&api_key_user(key_id))?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(SecretError(e.to_string())),
    }
}

const GATEWAY_TLS_KEY_USER: &str = "gateway-tls-key";

/// Private key (PEM) of the local API's self-signed TLS certificate.
pub fn gateway_tls_key() -> Result<Option<String>, SecretError> {
    match entry(GATEWAY_TLS_KEY_USER)?.get_password() {
        Ok(k) if !k.is_empty() => Ok(Some(k)),
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(SecretError(e.to_string())),
    }
}

pub fn set_gateway_tls_key(pem: &str) -> Result<(), SecretError> {
    entry(GATEWAY_TLS_KEY_USER)?
        .set_password(pem)
        .map_err(|e| SecretError(e.to_string()))
}
