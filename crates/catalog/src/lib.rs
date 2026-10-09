//! The catalog: what Demido Studio installs, pinned and checksummed.
//!
//! The data lives in `catalog/*.json` at the repository root so it can be reviewed and bumped
//! without touching code; it is compiled into the binaries so an install never depends on a
//! catalog fetched at runtime. [`plan`] turns a [`HardwareReport`](demido_hardware::HardwareReport)
//! into concrete choices.

mod data;
pub mod plan;

pub use data::*;
pub use plan::{BackendChoice, ModelRecommendation, backend_choices, default_backend, recommend_models, search_model};

use std::sync::OnceLock;

static RUNTIMES_JSON: &str = include_str!("../../../catalog/runtimes.json");
static MODELS_JSON: &str = include_str!("../../../catalog/models.json");

/// The catalog compiled into this binary.
pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| Catalog {
        runtimes: serde_json::from_str(RUNTIMES_JSON).expect("catalog/runtimes.json is valid"),
        models: serde_json::from_str(MODELS_JSON).expect("catalog/models.json is valid"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_parses_and_is_consistent() {
        let c = catalog();
        assert!(!c.runtimes.llama_cpp.variants.is_empty());
        for v in &c.runtimes.llama_cpp.variants {
            assert!(!v.assets.is_empty(), "{} has no assets", v.id);
            for a in &v.assets {
                assert_eq!(a.sha256.len(), 64, "{} checksum", a.name);
                assert!(a.size > 0);
            }
        }
        // Tiers must be ordered from the largest budget down so the first match wins.
        let mins: Vec<f64> = c.models.tiers.iter().map(|t| t.min_vram_gb).collect();
        assert!(mins.windows(2).all(|w| w[0] > w[1]), "tiers out of order: {mins:?}");
        for tier in &c.models.tiers {
            for family in &c.models.families {
                let pick = tier
                    .models
                    .get(&family.id)
                    .unwrap_or_else(|| panic!("tier {} lacks {}", tier.id, family.id));
                assert!(pick.file.ends_with(".gguf"));
                assert_eq!(pick.sha256.len(), 64);
                assert!(
                    (pick.size as f64 / demido_hardware::GIB as f64) < tier.min_vram_gb.max(3.0),
                    "{} does not fit its own tier {}",
                    pick.file,
                    tier.id
                );
            }
        }
    }

    #[test]
    fn search_models_are_pinned_and_picked_by_memory() {
        let search = &catalog().models.search;
        assert!(!search.models.is_empty());
        let mins: Vec<f64> = search.models.iter().map(|m| m.min_vram_gb).collect();
        assert!(
            mins.windows(2).all(|w| w[0] > w[1]),
            "search models out of order: {mins:?}"
        );
        for m in &search.models {
            assert!(m.file.ends_with(".gguf"));
            assert_eq!(m.sha256.len(), 64, "{}", m.file);
            assert!(m.size > 0 && m.relevance > 0.0 && m.relevance < 1.0);
            // The app's slot is 2048 tokens; a micro-batch is at most that.
            assert!(m.micro_batch.is_power_of_two() && m.micro_batch <= 2048, "{}", m.id);
            // unsloth when it publishes the model, otherwise the model's own maker.
            assert!(
                m.repo.starts_with("unsloth/") || m.repo.starts_with("Qwen/"),
                "{} is neither unsloth nor the vendor",
                m.repo
            );
        }
        assert_eq!(search.for_memory(Some(12.0)).id, "qwen3-embedding-0.6b");
        assert_eq!(search.for_memory(Some(8.0)).id, "embeddinggemma-300m");
        assert_eq!(search.for_memory(None).id, "embeddinggemma-300m");
    }

    #[test]
    fn every_starter_model_comes_with_its_bf16_projector() {
        let c = catalog();
        let picks = c
            .models
            .tiers
            .iter()
            .flat_map(|t| t.models.values())
            .chain(c.models.cpu_tiers.iter().flat_map(|t| t.models.values()));
        for pick in picks {
            let p = pick
                .projector
                .as_ref()
                .unwrap_or_else(|| panic!("{} has no projector", pick.file));
            assert!(p.file.contains("mmproj") && p.file.contains("BF16"), "{}", p.file);
            assert_eq!(p.sha256.len(), 64, "{}", p.file);
            assert!(p.size > 0);
            assert_eq!(pick.download_size(), pick.size + p.size);
            assert!(
                pick.projector_url()
                    .unwrap()
                    .starts_with(&format!("https://huggingface.co/{}/resolve/main/mmproj", pick.repo))
            );
        }
        // The smoke test only checks that a model answers.
        assert!(c.models.smoke_test.projector.is_none());
    }

    #[test]
    fn unsloth_is_the_preferred_publisher() {
        let c = catalog();
        for tier in &c.models.tiers {
            for pick in tier.models.values() {
                assert!(pick.repo.starts_with("unsloth/"), "{} is not unsloth", pick.repo);
            }
        }
    }
}
