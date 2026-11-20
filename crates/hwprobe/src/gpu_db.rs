//! GPU catalogue (`catalog/hardware.json`, ADR 0016), a small GPU-Z-style
//! database that complements what the drivers report: compute units, shader
//! ALUs, boost clock, peak memory bandwidth and whether the GPU is integrated.
//!
//! Matching: PCI `vendor:device` ID first (when the probe read one), then the
//! lower-case name substring patterns, first match wins. Values come from the
//! vendors' published specifications; unknown values stay `null` in the file.
//! Detection results always win over the catalogue.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::{GpuInfo, GpuVendor};

pub const HARDWARE_JSON: &str = include_str!("../../../catalog/hardware.json");

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct HardwareCatalog {
    pub version: u32,
    pub gpus: Vec<GpuSpec>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GpuSpec {
    /// Lower-case substrings of the reported GPU name.
    pub patterns: Vec<String>,
    /// `vendor:device` in lower-case hex, e.g. `10de:2204`.
    #[serde(default)]
    pub pci_ids: Vec<String>,
    pub vendor: GpuVendor,
    pub integrated: bool,
    #[serde(default)]
    pub unified: bool,
    pub compute_units: Option<u32>,
    /// `SM`, `CU`, `EU`, `Xe-core` or `GPU core`.
    pub unit: Option<String>,
    pub shader_cores: Option<u32>,
    pub boost_mhz: Option<u32>,
    /// `None` for integrated graphics (they share system RAM).
    pub bandwidth_gbs: Option<f32>,
    /// Smaller configurations of the same chip: up to N units → bandwidth.
    #[serde(default)]
    pub bandwidth_by_max_units: BTreeMap<String, f32>,
}

impl GpuSpec {
    fn bandwidth_for(&self, units: Option<u32>) -> Option<f32> {
        if let Some(units) = units {
            for (max, bw) in &self.bandwidth_by_max_units {
                if max.parse::<u32>().is_ok_and(|m| units <= m) {
                    return Some(*bw);
                }
            }
        }
        self.bandwidth_gbs
    }
}

pub fn catalog() -> &'static HardwareCatalog {
    static CATALOG: OnceLock<HardwareCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| match serde_json::from_str(HARDWARE_JSON) {
        Ok(c) => c,
        Err(e) => {
            // Covered by a unit test; never crash the probe over the catalogue.
            tracing::error!(error = %e, "catalog/hardware.json does not parse");
            HardwareCatalog {
                version: 0,
                gpus: Vec::new(),
            }
        }
    })
}

pub(crate) fn lookup(name: &str, pci_id: Option<&str>) -> Option<&'static GpuSpec> {
    let gpus = &catalog().gpus;
    if let Some(id) = pci_id.map(str::to_ascii_lowercase)
        && let Some(spec) = gpus.iter().find(|s| s.pci_ids.contains(&id))
    {
        return Some(spec);
    }
    let n = name.to_ascii_lowercase().replace(['-', '_'], " ");
    gpus.iter().find(|s| {
        s.patterns.iter().any(|p| {
            n.contains(p.as_str())
                // "m1" must not match "m10…" or similar model numbers
                && (s.vendor != GpuVendor::Apple || word_boundary_after(&n, p))
        })
    })
}

fn word_boundary_after(name: &str, pattern: &str) -> bool {
    name.match_indices(pattern).any(|(i, _)| {
        name[i + pattern.len()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_alphanumeric())
    })
}

/// Fills compute details the driver did not report. Never overrides measured
/// values; records `table` as the source when it filled anything.
pub(crate) fn enrich(gpu: &mut GpuInfo) {
    let Some(spec) = lookup(&gpu.name, gpu.pci_id.as_deref()) else {
        if gpu.fp32_tflops.is_none() {
            tracing::debug!(name = %gpu.name, pci_id = ?gpu.pci_id, "GPU not in catalog/hardware.json");
        }
        fill_tflops(gpu);
        return;
    };
    if spec.vendor != gpu.vendor && gpu.vendor != GpuVendor::Other {
        fill_tflops(gpu);
        return;
    }
    let mut filled = false;
    let mut set = |slot: &mut Option<u32>, value: Option<u32>| {
        if slot.is_none()
            && let Some(v) = value.filter(|&v| v > 0)
        {
            *slot = Some(v);
            filled = true;
        }
    };
    set(&mut gpu.compute_units, spec.compute_units);
    if spec.vendor == GpuVendor::Apple {
        // Apple: ALUs follow the actual core count (128 per core).
        set(
            &mut gpu.shader_cores,
            gpu.compute_units.or(spec.compute_units).map(|c| c * 128),
        );
    } else {
        set(&mut gpu.shader_cores, spec.shader_cores);
    }
    set(&mut gpu.clock_mhz, spec.boost_mhz);
    if gpu.memory_bandwidth_gbs.is_none()
        && let Some(bw) = spec.bandwidth_for(gpu.compute_units).filter(|&b| b > 0.0)
    {
        gpu.memory_bandwidth_gbs = Some(bw);
        filled = true;
    }
    if gpu.integrated.is_none() && !gpu.unified {
        gpu.integrated = Some(spec.integrated);
        filled = true;
    }
    fill_tflops(gpu);
    if filled {
        gpu.details_source = Some(match gpu.details_source.take() {
            Some(src) => format!("{src}+table"),
            None => "table".into(),
        });
    }
}

fn fill_tflops(gpu: &mut GpuInfo) {
    if gpu.fp32_tflops.is_none()
        && let (Some(cores), Some(mhz)) = (gpu.shader_cores, gpu.clock_mhz)
    {
        gpu.fp32_tflops = Some(cores as f32 * mhz as f32 * 2.0 / 1_000_000.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(vendor: GpuVendor, name: &str) -> GpuInfo {
        GpuInfo {
            vendor,
            name: name.into(),
            source: "test".into(),
            ..Default::default()
        }
    }

    fn pattern(name: &str) -> &'static str {
        lookup(name, None).unwrap().patterns[0].as_str()
    }

    #[test]
    fn catalogue_parses_and_is_consistent() {
        let c: HardwareCatalog =
            serde_json::from_str(HARDWARE_JSON).expect("catalog/hardware.json must parse");
        assert!(c.version >= 1);
        assert!(c.gpus.len() >= 100, "starter set");
        let mut ids = std::collections::HashSet::new();
        for g in &c.gpus {
            assert!(!g.patterns.is_empty());
            for p in &g.patterns {
                assert_eq!(p, &p.to_ascii_lowercase(), "patterns are lower-case");
            }
            for id in &g.pci_ids {
                assert!(ids.insert(id.clone()), "duplicate PCI id {id}");
                let (v, d) = id.split_once(':').expect("vendor:device");
                assert!(
                    v.len() == 4 && d.len() == 4 && u32::from_str_radix(d, 16).is_ok(),
                    "{id}"
                );
            }
            if g.integrated {
                assert_eq!(
                    g.bandwidth_gbs, None,
                    "{:?}: integrated GPUs share RAM",
                    g.patterns
                );
            }
            if let (Some(units), Some(cores)) = (g.compute_units, g.shader_cores) {
                assert!(cores >= units, "{:?}", g.patterns);
            }
        }
        for vendor in [
            GpuVendor::Nvidia,
            GpuVendor::Amd,
            GpuVendor::Intel,
            GpuVendor::Apple,
        ] {
            assert!(c.gpus.iter().any(|g| g.vendor == vendor), "{vendor:?}");
        }
    }

    #[test]
    fn matches_most_specific_entry() {
        assert_eq!(pattern("NVIDIA GeForce RTX 3090 Ti"), "rtx 3090 ti");
        assert_eq!(pattern("NVIDIA GeForce RTX 3090"), "rtx 3090");
        assert_eq!(pattern("AMD Radeon RX 7900 XTX"), "rx 7900 xtx");
        assert_eq!(pattern("Intel(R) HD Graphics 620"), "hd graphics 620");
        assert_eq!(pattern("Apple M2 Pro"), "m2 pro");
        assert_eq!(pattern("Apple M2"), "m2");
        assert!(lookup("Matrox G200", None).is_none());
    }

    #[test]
    fn pci_id_wins_over_name() {
        // Linux reports "HD Graphics 620" without the Intel prefix; the ID is exact.
        let spec = lookup("Some renamed adapter", Some("8086:5916")).unwrap();
        assert_eq!(spec.patterns[0], "hd graphics 620");
        assert_eq!(
            lookup("x", Some("10DE:2204")).unwrap().patterns[0],
            "rtx 3090"
        );
    }

    #[test]
    fn enriches_from_table_without_overriding_driver_values() {
        let mut g = gpu(GpuVendor::Nvidia, "NVIDIA GeForce RTX 3090");
        g.compute_units = Some(82);
        g.clock_mhz = Some(2100);
        g.details_source = Some("nvml".into());
        enrich(&mut g);
        assert_eq!(g.compute_units, Some(82));
        assert_eq!(g.clock_mhz, Some(2100));
        assert_eq!(g.shader_cores, Some(10_496));
        assert_eq!(g.memory_bandwidth_gbs, Some(936.0));
        assert_eq!(g.integrated, Some(false));
        assert!((g.fp32_tflops.unwrap() - 44.1).abs() < 0.1);
        assert_eq!(g.details_source.as_deref(), Some("nvml+table"));
    }

    #[test]
    fn integrated_graphics_from_table() {
        let mut hd = gpu(GpuVendor::Intel, "Intel(R) HD Graphics 620");
        enrich(&mut hd);
        assert_eq!(hd.integrated, Some(true));
        assert!(hd.is_integrated());
        assert_eq!(hd.compute_units, Some(24));
        assert_eq!(hd.memory_bandwidth_gbs, None);

        let mut apu = gpu(GpuVendor::Amd, "AMD Radeon 780M Graphics");
        enrich(&mut apu);
        assert_eq!(apu.integrated, Some(true));
        assert_eq!(apu.compute_units, Some(12));

        let mut arc = gpu(GpuVendor::Intel, "Intel(R) Arc(TM) A770 Graphics");
        enrich(&mut arc);
        assert_eq!(arc.integrated, Some(false));
        assert_eq!(arc.memory_bandwidth_gbs, Some(560.0));
    }

    #[test]
    fn apple_uses_real_core_count() {
        let mut m3max = gpu(GpuVendor::Apple, "Apple M3 Max");
        m3max.unified = true;
        m3max.compute_units = Some(30);
        enrich(&mut m3max);
        assert_eq!(m3max.shader_cores, Some(30 * 128));
        assert_eq!(m3max.memory_bandwidth_gbs, Some(300.0));
        assert_eq!(m3max.integrated, None, "unified GPUs are not 'integrated'");

        let mut full = gpu(GpuVendor::Apple, "Apple M3 Max");
        full.unified = true;
        full.compute_units = Some(40);
        enrich(&mut full);
        assert_eq!(full.memory_bandwidth_gbs, Some(400.0));
    }
}
