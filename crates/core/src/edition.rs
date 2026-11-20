//! Product edition (ADR 0035): the free open-source Lokalomat or Lokalomat Pro.
//!
//! This repository only builds the free edition. The closed-source Pro build
//! lives in a separate repository and reports `Edition::Pro` instead; the UI
//! reads the edition to decide whether to show the Pro banner.

use serde::Serialize;

use crate::catalog::Modality;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    Free,
    Pro,
}

impl Edition {
    /// The edition of this build.
    pub const fn current() -> Self {
        Edition::Free
    }

    pub const fn is_pro(self) -> bool {
        matches!(self, Edition::Pro)
    }

    /// The kinds of models this edition offers in its catalog: chat and decision
    /// models in the free edition, every kind in Pro.
    pub fn modalities(self) -> &'static [Modality] {
        match self {
            Edition::Free => &[Modality::Llm, Modality::Decision],
            Edition::Pro => &Modality::ALL,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_source_build_is_free() {
        assert_eq!(Edition::current(), Edition::Free);
        assert!(!Edition::current().is_pro());
    }

    #[test]
    fn free_catalog_has_only_chat_and_decision_models() {
        let all = crate::catalog::Catalog::embedded().unwrap();
        let mut free = all.clone();
        free.retain_modalities(Edition::Free.modalities());
        assert!(!free.models.is_empty());
        assert!(free.models.len() < all.models.len());
        assert!(
            free.models
                .iter()
                .all(|m| matches!(m.modality, Modality::Llm | Modality::Decision))
        );
        let mut pro = all.clone();
        pro.retain_modalities(Edition::Pro.modalities());
        assert_eq!(pro, all, "Pro keeps the whole list");
    }

    #[test]
    fn serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Edition::Pro).unwrap(), "\"pro\"");
    }
}
