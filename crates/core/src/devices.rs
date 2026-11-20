//! Phones paired with the local API gateway (PRD §15 item 12). Only metadata
//! lives in the database; each device's access key is kept in the OS
//! credential store (`secrets::*_device_key`).

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::{Db, DbError, now_secs};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// Random id, also the user name of the key in the credential store.
    pub id: String,
    pub name: String,
    /// The phone's Ed25519 public key (base64url).
    pub public_key: String,
    pub paired_at: i64,
    pub last_seen: Option<i64>,
}

pub struct Devices<'a> {
    db: &'a Db,
}

impl<'a> Devices<'a> {
    pub fn new(db: &'a Db) -> Self {
        Devices { db }
    }

    pub fn list(&self) -> Result<Vec<Device>, DbError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, public_key, paired_at, last_seen FROM paired_devices ORDER BY paired_at DESC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Device {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    public_key: r.get(2)?,
                    paired_at: r.get(3)?,
                    last_seen: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn insert(&self, id: &str, name: &str, public_key: &str) -> Result<Device, DbError> {
        let now = now_secs();
        self.db.conn().execute(
            "INSERT INTO paired_devices (id, name, public_key, paired_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, name, public_key, now],
        )?;
        Ok(Device {
            id: id.to_owned(),
            name: name.to_owned(),
            public_key: public_key.to_owned(),
            paired_at: now,
            last_seen: None,
        })
    }

    pub fn delete(&self, id: &str) -> Result<bool, DbError> {
        Ok(self
            .db
            .conn()
            .execute("DELETE FROM paired_devices WHERE id = ?1", params![id])?
            > 0)
    }

    pub fn touch(&self, id: &str) -> Result<(), DbError> {
        self.db.conn().execute(
            "UPDATE paired_devices SET last_seen = ?2 WHERE id = ?1",
            params![id, now_secs()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let d = Devices::new(&db);
        d.insert("abc", "Pixel", "pk").unwrap();
        d.touch("abc").unwrap();
        let all = d.list().unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].last_seen.is_some());
        assert!(d.delete("abc").unwrap());
        assert!(!d.delete("abc").unwrap());
    }
}
