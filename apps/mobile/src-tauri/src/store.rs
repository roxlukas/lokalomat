//! What the phone remembers after pairing: its Ed25519 identity (seed) and
//! the credentials of the paired computers (address, pinned certificate,
//! access key), one of them in use. On desktop builds of this client they go
//! to the OS credential store; on Android to the app's private storage, which
//! other apps cannot read (ADR 0020 – an Android Keystore wrapper is a follow-up).

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
    /// Older versions kept one computer here; [`Saved::migrated`] moves it to `computers`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<Credentials>,
    #[serde(default)]
    pub computers: Vec<Credentials>,
    /// Certificate fingerprint of the computer in use (it identifies a computer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
}

impl Saved {
    /// Files written by earlier versions hold a single computer in `credentials`.
    pub fn migrated(mut self) -> Self {
        if let Some(old) = self.credentials.take() {
            if self.active.is_none() {
                self.active = Some(old.fingerprint.clone());
            }
            if !self
                .computers
                .iter()
                .any(|c| c.fingerprint == old.fingerprint)
            {
                self.computers.push(old);
            }
        }
        self
    }

    /// The computer in use; the first one when `active` points nowhere.
    pub fn active_credentials(&self) -> Option<Credentials> {
        self.active
            .as_deref()
            .and_then(|id| self.computers.iter().find(|c| c.fingerprint == id))
            .or_else(|| self.computers.first())
            .cloned()
    }

    /// Adds a computer (pairing again with the same one replaces its key) and makes it the one in use.
    pub fn add(&mut self, creds: Credentials) {
        self.active = Some(creds.fingerprint.clone());
        match self
            .computers
            .iter_mut()
            .find(|c| c.fingerprint == creds.fingerprint)
        {
            Some(existing) => *existing = creds,
            None => self.computers.push(creds),
        }
    }

    /// Switches to a paired computer; false when there is none with this id.
    pub fn select(&mut self, id: &str) -> bool {
        let known = self.computers.iter().any(|c| c.fingerprint == id);
        if known {
            self.active = Some(id.to_owned());
        }
        known
    }

    /// Forgets a computer; when it was in use the first remaining one takes over.
    pub fn remove(&mut self, id: &str) {
        self.computers.retain(|c| c.fingerprint != id);
        if self.active.as_deref() == Some(id) {
            self.active = self.computers.first().map(|c| c.fingerprint.clone());
        }
    }
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
    fn load_raw(&self) -> Saved {
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
    fn load_raw(&self) -> Saved {
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

    pub fn load(&self) -> Saved {
        self.load_raw().migrated()
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

    /// The computer in use.
    pub fn active_credentials(&self) -> Option<Credentials> {
        self.load().active_credentials()
    }

    pub fn add(&self, creds: Credentials) -> Result<(), String> {
        let mut saved = self.load();
        saved.add(creds);
        self.save(&saved)
    }

    /// Switches the computer in use; `None` when there is no such computer.
    pub fn select(&self, id: &str) -> Result<Option<Credentials>, String> {
        let mut saved = self.load();
        if !saved.select(id) {
            return Ok(None);
        }
        self.save(&saved)?;
        Ok(saved.active_credentials())
    }

    /// Forgets a computer; returns the one in use afterwards.
    pub fn remove(&self, id: &str) -> Result<Option<Credentials>, String> {
        let mut saved = self.load();
        saved.remove(id);
        self.save(&saved)?;
        Ok(saved.active_credentials())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn computer(name: &str, fingerprint: &str) -> Credentials {
        Credentials {
            host: "192.168.1.10".into(),
            port: 47821,
            fingerprint: fingerprint.into(),
            device_id: "d".into(),
            access_key: "k".into(),
            computer_name: name.into(),
        }
    }

    #[test]
    fn an_old_file_with_one_computer_keeps_working() {
        let old = r#"{"identity":"abc","credentials":{"host":"h","port":1,"fingerprint":"fp","device_id":"d","access_key":"k","computer_name":"NIAGARA"}}"#;
        let saved: Saved = serde_json::from_str::<Saved>(old).unwrap().migrated();
        assert_eq!(saved.computers.len(), 1);
        assert_eq!(saved.active.as_deref(), Some("fp"));
        assert_eq!(saved.active_credentials().unwrap().computer_name, "NIAGARA");
        // Saved again, the old field is gone.
        assert!(
            !serde_json::to_string(&saved)
                .unwrap()
                .contains("\"credentials\"")
        );
    }

    #[test]
    fn adding_makes_the_new_computer_active_and_pairing_again_replaces_it() {
        let mut saved = Saved::default();
        saved.add(computer("A", "fa"));
        saved.add(computer("B", "fb"));
        assert_eq!(saved.active.as_deref(), Some("fb"));
        assert_eq!(saved.computers.len(), 2);
        let mut again = computer("A renamed", "fa");
        again.access_key = "new".into();
        saved.add(again);
        assert_eq!(saved.computers.len(), 2);
        assert_eq!(saved.active.as_deref(), Some("fa"));
        assert_eq!(saved.active_credentials().unwrap().access_key, "new");
    }

    #[test]
    fn select_only_knows_paired_computers() {
        let mut saved = Saved::default();
        saved.add(computer("A", "fa"));
        saved.add(computer("B", "fb"));
        assert!(saved.select("fa"));
        assert_eq!(saved.active_credentials().unwrap().computer_name, "A");
        assert!(!saved.select("nope"));
        assert_eq!(saved.active.as_deref(), Some("fa"));
    }

    #[test]
    fn removing_the_active_computer_hands_over_to_another() {
        let mut saved = Saved::default();
        saved.add(computer("A", "fa"));
        saved.add(computer("B", "fb"));
        saved.remove("fb");
        assert_eq!(saved.active.as_deref(), Some("fa"));
        saved.remove("fa");
        assert!(saved.active.is_none());
        assert!(saved.active_credentials().is_none());
        // Removing something unknown changes nothing.
        saved.remove("zzz");
    }
}
