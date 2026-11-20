//! Where the gateway keeps its secrets: device access keys and the TLS
//! private key go to the OS credential store ([`OsKeyStore`]); tests use
//! [`MemoryKeyStore`].

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

pub trait KeyStore: Send + Sync + 'static {
    fn device_key(&self, device_id: &str) -> Option<String>;
    fn set_device_key(&self, device_id: &str, key: &str) -> Result<(), String>;
    fn delete_device_key(&self, device_id: &str) -> Result<(), String>;
    /// PKCS#8 DER of the TLS key, base64.
    fn tls_key(&self) -> Option<String>;
    fn set_tls_key(&self, key: &str) -> Result<(), String>;
}

pub struct OsKeyStore;

impl KeyStore for OsKeyStore {
    fn device_key(&self, device_id: &str) -> Option<String> {
        lokalsi_core::secrets::device_key(device_id).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "cannot read a device key");
            None
        })
    }

    fn set_device_key(&self, device_id: &str, key: &str) -> Result<(), String> {
        lokalsi_core::secrets::set_device_key(device_id, key).map_err(|e| e.to_string())
    }

    fn delete_device_key(&self, device_id: &str) -> Result<(), String> {
        lokalsi_core::secrets::delete_device_key(device_id).map_err(|e| e.to_string())
    }

    fn tls_key(&self) -> Option<String> {
        lokalsi_core::secrets::gateway_tls_key().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "cannot read the gateway TLS key");
            None
        })
    }

    fn set_tls_key(&self, key: &str) -> Result<(), String> {
        lokalsi_core::secrets::set_gateway_tls_key(key).map_err(|e| e.to_string())
    }
}

#[derive(Default)]
pub struct MemoryKeyStore {
    keys: Mutex<HashMap<String, String>>,
}

impl MemoryKeyStore {
    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.keys.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl KeyStore for MemoryKeyStore {
    fn device_key(&self, device_id: &str) -> Option<String> {
        self.map().get(&format!("device:{device_id}")).cloned()
    }

    fn set_device_key(&self, device_id: &str, key: &str) -> Result<(), String> {
        self.map()
            .insert(format!("device:{device_id}"), key.to_owned());
        Ok(())
    }

    fn delete_device_key(&self, device_id: &str) -> Result<(), String> {
        self.map().remove(&format!("device:{device_id}"));
        Ok(())
    }

    fn tls_key(&self) -> Option<String> {
        self.map().get("tls").cloned()
    }

    fn set_tls_key(&self, key: &str) -> Result<(), String> {
        self.map().insert("tls".into(), key.to_owned());
        Ok(())
    }
}

/// The gateway's certificate (DER, public, in `cert_path`) and private key
/// (in the key store). Generated on first use; regenerated when either is
/// missing, which invalidates the pin on paired phones (they pair again).
pub fn load_or_create_identity(
    cert_path: &Path,
    keys: &dyn KeyStore,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let cert = std::fs::read(cert_path).ok();
    let key = keys.tls_key().and_then(|k| STANDARD.decode(k).ok());
    if let (Some(cert), Some(key)) = (cert, key)
        && crate::tls::server_config(cert.clone(), key.clone()).is_ok()
    {
        return Ok((cert, key));
    }
    let (cert, key) = crate::tls::generate().map_err(|e| e.to_string())?;
    keys.set_tls_key(&STANDARD.encode(&key))?;
    if let Some(dir) = cert_path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(cert_path, &cert).map_err(|e| e.to_string())?;
    tracing::info!("new TLS certificate created for the local API");
    Ok((cert, key))
}
