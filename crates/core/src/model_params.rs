//! Default parameters of a chat model, set in the Inspector (ADR 0036).
//!
//! One row per model variant in `model_settings`; a missing row or a `None`
//! field means "Auto": the engine's or model's own default. Requests that
//! bring their own values (the local OpenAI-compatible server) keep them.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::db::{Db, DbError, now_secs};

/// Highest temperature the Inspector offers.
pub const TEMPERATURE_MAX: f32 = 2.0;
/// Highest top-k the Inspector offers (0 turns top-k off).
pub const TOP_K_MAX: u32 = 200;
/// `gpu_layers` meaning "every layer on the GPU" (llama.cpp caps `-ngl` at the model's count).
pub const GPU_LAYERS_ALL: u32 = 999;

/// Defaults of one model variant; `None` = Auto.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelParams {
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    /// Layers on the GPU (`-ngl`, Expert only): 0 = processor only,
    /// [`GPU_LAYERS_ALL`] = all; `None` = the engine decides.
    pub gpu_layers: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum ModelParamsError {
    #[error("{0} is out of range")]
    OutOfRange(&'static str),
    #[error(transparent)]
    Db(#[from] DbError),
}

impl From<rusqlite::Error> for ModelParamsError {
    fn from(e: rusqlite::Error) -> Self {
        ModelParamsError::Db(DbError::Sql(e))
    }
}

impl ModelParams {
    /// Every field is Auto.
    pub fn is_auto(&self) -> bool {
        *self == ModelParams::default()
    }

    pub fn validate(&self) -> Result<(), ModelParamsError> {
        let within =
            |v: Option<f32>, max: f32| v.is_none_or(|v| v.is_finite() && (0.0..=max).contains(&v));
        if !within(self.temperature, TEMPERATURE_MAX) {
            return Err(ModelParamsError::OutOfRange("temperature"));
        }
        if !within(self.top_p, 1.0) {
            return Err(ModelParamsError::OutOfRange("top_p"));
        }
        if self.top_k.is_some_and(|k| k > TOP_K_MAX) {
            return Err(ModelParamsError::OutOfRange("top_k"));
        }
        if self.gpu_layers.is_some_and(|n| n > GPU_LAYERS_ALL) {
            return Err(ModelParamsError::OutOfRange("gpu_layers"));
        }
        Ok(())
    }
}

pub struct ModelSettings<'a> {
    db: &'a Db,
}

impl<'a> ModelSettings<'a> {
    pub fn new(db: &'a Db) -> Self {
        ModelSettings { db }
    }

    /// Saved defaults of a variant; Auto when nothing is saved. A row that
    /// cannot be read is treated as Auto (and logged), never as an error.
    pub fn get(&self, model_id: &str, variant_id: &str) -> Result<ModelParams, DbError> {
        let json: Option<String> = self
            .db
            .conn()
            .query_row(
                "SELECT params FROM model_settings WHERE model_id = ?1 AND variant_id = ?2",
                params![model_id, variant_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(match json {
            Some(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!(model_id, variant_id, error = %e, "stored model parameters are invalid, using Auto");
                ModelParams::default()
            }),
            None => ModelParams::default(),
        })
    }

    /// Saves the defaults of a variant; all Auto removes the row.
    pub fn set(
        &self,
        model_id: &str,
        variant_id: &str,
        values: &ModelParams,
    ) -> Result<(), ModelParamsError> {
        values.validate()?;
        let conn = self.db.conn();
        if values.is_auto() {
            conn.execute(
                "DELETE FROM model_settings WHERE model_id = ?1 AND variant_id = ?2",
                params![model_id, variant_id],
            )?;
        } else {
            let json = serde_json::to_string(values).map_err(DbError::Json)?;
            conn.execute(
                "INSERT INTO model_settings (model_id, variant_id, params, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(model_id, variant_id) DO UPDATE SET params = excluded.params, updated_at = excluded.updated_at",
                params![model_id, variant_id, json, now_secs()],
            )?;
        }
        tracing::info!(model_id, variant_id, ?values, "model parameters saved");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    #[test]
    fn missing_row_is_auto() {
        let db = db();
        let p = ModelSettings::new(&db).get("m", "v").unwrap();
        assert!(p.is_auto());
    }

    #[test]
    fn set_get_and_back_to_auto() {
        let db = db();
        let store = ModelSettings::new(&db);
        let p = ModelParams {
            temperature: Some(0.2),
            top_p: None,
            top_k: Some(40),
            gpu_layers: Some(0),
        };
        store.set("m", "v", &p).unwrap();
        assert_eq!(store.get("m", "v").unwrap(), p);
        assert!(store.get("m", "other").unwrap().is_auto(), "per variant");
        store.set("m", "v", &ModelParams::default()).unwrap();
        assert!(store.get("m", "v").unwrap().is_auto());
        let rows: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM model_settings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0, "all Auto removes the row");
    }

    #[test]
    fn out_of_range_is_rejected() {
        let db = db();
        let store = ModelSettings::new(&db);
        for bad in [
            ModelParams {
                temperature: Some(2.5),
                ..Default::default()
            },
            ModelParams {
                temperature: Some(f32::NAN),
                ..Default::default()
            },
            ModelParams {
                top_p: Some(-0.1),
                ..Default::default()
            },
            ModelParams {
                top_k: Some(TOP_K_MAX + 1),
                ..Default::default()
            },
            ModelParams {
                gpu_layers: Some(GPU_LAYERS_ALL + 1),
                ..Default::default()
            },
        ] {
            assert!(
                matches!(
                    store.set("m", "v", &bad),
                    Err(ModelParamsError::OutOfRange(_))
                ),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn corrupt_row_reads_as_auto() {
        let db = db();
        db.conn()
            .execute(
                "INSERT INTO model_settings (model_id, variant_id, params, updated_at) VALUES ('m', 'v', '{oops', 0)",
                [],
            )
            .unwrap();
        assert!(ModelSettings::new(&db).get("m", "v").unwrap().is_auto());
    }
}
