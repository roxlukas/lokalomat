//! What the phone remembers after pairing: its Ed25519 identity (seed) and
//! the credentials for the computer (address, pinned certificate, access
//! key). On desktop builds of this client they go to the OS credential
//! store; on Android to the app's private storage, which other apps cannot
//! read (ADR 0020 – an Android Keystore wrapper is a follow-up).

use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use lokalsi_api_gateway::client::Credentials;
use lokalsi_api_gateway::pairing::DeviceIdentity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Saved {
    /// Ed25519 seed, base64url.
    pub identity: Option<String>,
    pub credentials: Option<Credentials>,
}

pub struct Store {
    #[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
    path: PathBuf,
}

impl Store {
    pub fn new(data_dir: &Path) -> Self {
        Store {
            path: data_dir.join("pairing.json"),
        }
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub fn load(&self) -> Saved {
        std::fs::read(&self.path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub fn save(&self, saved: &Saved) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let bytes = serde_json::to_vec(saved).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, bytes).map_err(|e| e.to_string())
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn entry() -> Result<keyring::Entry, String> {
        keyring::Entry::new("Lokalomat Mobile", "pairing").map_err(|e| e.to_string())
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn load(&self) -> Saved {
        Self::entry()
            .ok()
            .and_then(|e| e.get_password().ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn save(&self, saved: &Saved) -> Result<(), String> {
        let json = serde_json::to_string(saved).map_err(|e| e.to_string())?;
        Self::entry()?
            .set_password(&json)
            .map_err(|e| e.to_string())
    }

    /// The phone's key; created on first use and kept across pairings.
    pub fn identity(&self) -> Result<DeviceIdentity, String> {
        let mut saved = self.load();
        if let Some(seed) = saved
            .identity
            .as_deref()
            .and_then(|s| URL_SAFE_NO_PAD.decode(s).ok())
            .and_then(|b| <[u8; 32]>::try_from(b).ok())
        {
            return Ok(DeviceIdentity::from_seed(seed));
        }
        let id = DeviceIdentity::generate();
        saved.identity = Some(URL_SAFE_NO_PAD.encode(id.seed()));
        self.save(&saved)?;
        Ok(id)
    }

    pub fn credentials(&self) -> Option<Credentials> {
        self.load().credentials
    }

    pub fn set_credentials(&self, creds: Option<Credentials>) -> Result<(), String> {
        let mut saved = self.load();
        saved.credentials = creds;
        self.save(&saved)
    }
}
