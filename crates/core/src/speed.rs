//! Catalog → speed estimate glue (docs/adr/0016-speed-tiers.md): turns a model
//! variant into the hardware-independent [`Workload`] used by `lokalsi_hwprobe`.

use lokalsi_hwprobe::{FitReport, MemoryBudget, SpeedReport, WorkKind, Workload, estimate};

use crate::catalog::{Modality, Model, Variant};

/// Engines whose narration is generated token by token like an LLM.
fn tts_kind(engine: &str) -> WorkKind {
    if engine == "piper" {
        WorkKind::Tts
    } else {
        WorkKind::TtsAutoregressive
    }
}

pub fn workload(model: &Model, variant: &Variant) -> Workload {
    let bytes = variant.total_size();
    let kind = match model.modality {
        Modality::Llm => WorkKind::Llm,
        Modality::Image => WorkKind::Image,
        Modality::Stt => WorkKind::Stt,
        Modality::Tts => tts_kind(&model.engine),
        Modality::Music => WorkKind::Music,
        // Not shown (see `speed_of`); a decision is one prompt pass.
        Modality::Decision => WorkKind::Llm,
    };
    // Prefer the catalog's parameter count; otherwise derive it from the file
    // size and the quantisation named in the variant id/label.
    let params_b = model.params_b.unwrap_or_else(|| {
        let label = format!("{} {}", variant.id, variant.label);
        lokalsi_hwprobe::params_from_size(bytes, &label)
    });
    Workload {
        kind,
        weights_bytes: bytes,
        params_b,
        active_params_b: model.active_params_b,
    }
}

/// Speed of `variant` under `budget`, when the budget carries a speed basis.
pub fn speed_of(
    model: &Model,
    variant: &Variant,
    budget: &MemoryBudget,
    fit: &FitReport,
) -> Option<SpeedReport> {
    // A decision is a single prompt pass of a small model: tokens per second
    // say nothing useful about it, so no speed level is shown.
    if model.modality == Modality::Decision {
        return None;
    }
    let basis = budget.speed.as_ref()?;
    Some(estimate(&workload(model, variant), basis, fit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    #[test]
    fn workloads_from_catalog() {
        let catalog = Catalog::embedded().unwrap();
        let find = |id: &str| catalog.models.iter().find(|m| m.id == id).unwrap();

        let qwen = find("qwen3.6-35b-a3b");
        let w = workload(qwen, &qwen.variants[0]);
        assert_eq!(w.kind, WorkKind::Llm);
        assert_eq!(w.active_params_b, Some(3.0));

        let turbo = find("whisper-large-v3-turbo");
        let q5 = turbo.variants.iter().find(|v| v.id == "q5_0").unwrap();
        let w = workload(turbo, q5);
        assert!((0.7..1.0).contains(&w.params_b), "{w:?}");

        assert_eq!(
            workload(
                find("piper-pl-darkman"),
                &find("piper-pl-darkman").variants[0]
            )
            .kind,
            WorkKind::Tts
        );
        let cb = find("chatterbox-multilingual");
        assert_eq!(
            workload(cb, &cb.variants[0]).kind,
            WorkKind::TtsAutoregressive
        );
    }
}
