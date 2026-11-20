//! `engine.json` (PRD §4.3): what an engine can do, which release builds exist
//! for which platform/backend, and how to launch it. Sizes and sha256 sums of
//! release assets are filled by `tools/catalog-sync` from the GitHub API.

use serde::{Deserialize, Serialize};

use crate::{Capability, EngineKind};
use lokalsi_hwprobe::{Backend, GpuVendor, HardwareProfile};

pub const LLAMA_CPP_JSON: &str = include_str!("../../../catalog/engines/llama-cpp.json");
pub const MCP_GRAFANA_JSON: &str = include_str!("../../../catalog/engines/mcp-grafana.json");
pub const WHISPER_CPP_JSON: &str = include_str!("../../../catalog/engines/whisper-cpp.json");
pub const SD_CPP_JSON: &str = include_str!("../../../catalog/engines/sd-cpp.json");
pub const UV_JSON: &str = include_str!("../../../catalog/engines/uv.json");
/// Private Node.js for agents installed from npm (M5c, ADR 0026).
pub const NODE_JSON: &str = include_str!("../../../catalog/engines/node.json");
/// Agent binaries from GitHub releases (M5c, ADR 0026).
pub const GOOSE_JSON: &str = include_str!("../../../catalog/engines/goose.json");
pub const OPENCODE_JSON: &str = include_str!("../../../catalog/engines/opencode.json");
/// Factory's Droid: a single binary per platform, closed source (ADR 0028).
pub const DROID_JSON: &str = include_str!("../../../catalog/engines/droid.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineLicense {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallSource {
    /// `github-release` (assets of `repo` at `tag`) or `url` (files under
    /// `base_url`, checked against its `SHASUMS256.txt`, e.g. nodejs.org).
    pub source: String,
    #[serde(default)]
    pub repo: String,
    pub tag: String,
    /// `url` sources: the folder the files are in, ending with `/`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// `url` sources: where catalog-sync takes the sha256 from, the folder's
    /// `SHASUMS256.txt` (default) or `per-file` (`<file>.sha256`, Factory).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksums: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildFile {
    pub name: String,
    pub size: u64,
    /// Lower-case hex from the GitHub asset digest; empty until synced.
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    pub id: String,
    /// `windows-x64`, `macos-arm64`, … (as in the model catalog).
    pub platform: String,
    pub backend: Backend,
    /// Minimum NVIDIA driver major version (CUDA builds only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_driver: Option<u32>,
    /// Minimum glibc version, e.g. `2.43` (Linux builds made on a newer Ubuntu).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_glibc: Option<String>,
    /// Minimum macOS major version (builds made with a newer deployment target).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_macos: Option<u32>,
    pub files: Vec<BuildFile>,
}

/// What the running system can execute, for builds with `min_glibc` / `min_macos`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostInfo {
    /// `(major, minor)` of glibc on Linux.
    pub glibc: Option<(u32, u32)>,
    /// macOS major version.
    pub macos: Option<u32>,
}

impl HostInfo {
    /// Detects the running system once.
    pub fn current() -> Self {
        static HOST: std::sync::OnceLock<HostInfo> = std::sync::OnceLock::new();
        *HOST.get_or_init(|| HostInfo {
            glibc: detect_glibc(),
            macos: detect_macos(),
        })
    }

    /// True when `build` can run here. Unknown versions count as too old.
    pub fn can_run(&self, build: &Build) -> bool {
        let glibc_ok =
            build
                .min_glibc
                .as_deref()
                .is_none_or(|min| match (parse_version(min), self.glibc) {
                    (Some(min), Some(have)) => have >= min,
                    _ => false,
                });
        let macos_ok = build
            .min_macos
            .is_none_or(|min| self.macos.is_some_and(|have| have >= min));
        glibc_ok && macos_ok
    }
}

fn parse_version(v: &str) -> Option<(u32, u32)> {
    let mut it = v.trim().split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}

#[cfg(target_os = "linux")]
fn detect_glibc() -> Option<(u32, u32)> {
    // SAFETY: gnu_get_libc_version returns a static, NUL-terminated string.
    let v = unsafe { std::ffi::CStr::from_ptr(libc::gnu_get_libc_version()) };
    parse_version(&v.to_string_lossy())
}

#[cfg(not(target_os = "linux"))]
fn detect_glibc() -> Option<(u32, u32)> {
    None
}

#[cfg(target_os = "macos")]
fn detect_macos() -> Option<u32> {
    let out = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    parse_version(&String::from_utf8_lossy(&out.stdout)).map(|(major, _)| major)
}

#[cfg(not(target_os = "macos"))]
fn detect_macos() -> Option<u32> {
    None
}

impl Build {
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    pub cmd: String,
    /// Placeholders: `{model_path}`, `{port}`, `{token}`, `{ctx}`.
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineManifest {
    pub id: String,
    pub name: String,
    pub capabilities: Vec<Capability>,
    pub kind: EngineKind,
    pub version: String,
    pub license: EngineLicense,
    pub install: InstallSource,
    pub builds: Vec<Build>,
    pub launch: Launch,
    pub health: Health,
}

impl EngineManifest {
    pub fn llama_cpp() -> Self {
        serde_json::from_str(LLAMA_CPP_JSON)
            .expect("catalog/engines/llama-cpp.json is valid (tested)")
    }

    pub fn whisper_cpp() -> Self {
        serde_json::from_str(WHISPER_CPP_JSON)
            .expect("catalog/engines/whisper-cpp.json is valid (tested)")
    }

    pub fn sd_cpp() -> Self {
        serde_json::from_str(SD_CPP_JSON).expect("catalog/engines/sd-cpp.json is valid (tested)")
    }

    /// `uv`, which creates the Python environments of TTS engines (ADR 0018).
    pub fn uv() -> Self {
        serde_json::from_str(UV_JSON).expect("catalog/engines/uv.json is valid (tested)")
    }

    /// Built-in programs that install themselves (plug and play): MCP servers,
    /// the private Node.js and agent binaries.
    pub fn builtin(name: &str) -> Option<Self> {
        match name {
            "mcp-grafana" => serde_json::from_str(MCP_GRAFANA_JSON).ok(),
            "node" => serde_json::from_str(NODE_JSON).ok(),
            "goose" => serde_json::from_str(GOOSE_JSON).ok(),
            "opencode" => serde_json::from_str(OPENCODE_JSON).ok(),
            "droid" => serde_json::from_str(DROID_JSON).ok(),
            "uv" => serde_json::from_str(UV_JSON).ok(),
            _ => None,
        }
    }

    pub fn build(&self, id: &str) -> Option<&Build> {
        self.builds.iter().find(|b| b.id == id)
    }

    /// `https://github.com/<repo>/releases/download/<tag>/<name>`, or
    /// `<base_url><name>` for `url` sources.
    pub fn asset_url(&self, file: &BuildFile) -> String {
        match (
            self.install.source.as_str(),
            self.install.base_url.as_deref(),
        ) {
            ("url", Some(base)) => format!("{base}{}", file.name),
            _ => format!(
                "https://github.com/{}/releases/download/{}/{}",
                self.install.repo, self.install.tag, file.name
            ),
        }
    }

    /// Builds for `platform`, best first for this hardware: CUDA (newest the
    /// driver supports) → Metal → Vulkan → CPU.
    pub fn builds_for(&self, platform: &str, profile: Option<&HardwareProfile>) -> Vec<&Build> {
        self.builds_for_host(platform, profile, &HostInfo::current())
    }

    /// [`Self::builds_for`] for an explicit host (tests).
    pub fn builds_for_host(
        &self,
        platform: &str,
        profile: Option<&HardwareProfile>,
        host: &HostInfo,
    ) -> Vec<&Build> {
        let driver = profile.and_then(nvidia_driver_major);
        let has_nvidia = driver.is_some()
            || profile.is_some_and(|p| p.gpus.iter().any(|g| g.vendor == GpuVendor::Nvidia));
        let backends: Vec<Backend> = profile
            .map(|p| p.backends.clone())
            .unwrap_or_else(|| vec![Backend::Cpu]);
        let rank = |b: &Build| -> Option<u32> {
            if !backends.contains(&b.backend) && b.backend != Backend::Cpu {
                return None;
            }
            match b.backend {
                Backend::Cuda if has_nvidia => match (b.min_driver, driver) {
                    (Some(min), Some(d)) if d >= min => Some(0),
                    (Some(_), _) => None,
                    (None, _) => Some(1),
                },
                Backend::Cuda => None,
                Backend::Metal => Some(2),
                Backend::Vulkan => Some(3),
                Backend::Cpu => Some(4),
            }
        };
        let mut builds: Vec<(&Build, u32)> = self
            .builds
            .iter()
            .filter(|b| b.platform == platform && host.can_run(b))
            .filter_map(|b| rank(b).map(|r| (b, r)))
            .collect();
        builds.sort_by_key(|(_, r)| *r);
        builds.into_iter().map(|(b, _)| b).collect()
    }

    /// Files without a synced size or sha256.
    pub fn unsynced_files(&self) -> Vec<String> {
        self.builds
            .iter()
            .flat_map(|b| b.files.iter())
            .filter(|f| f.size == 0 || !lokalsi_core::catalog::is_sha256(&f.sha256))
            .map(|f| f.name.clone())
            .collect()
    }
}

fn nvidia_driver_major(p: &HardwareProfile) -> Option<u32> {
    p.gpus
        .iter()
        .filter(|g| g.vendor == GpuVendor::Nvidia)
        .filter_map(|g| g.driver.as_deref())
        .filter_map(|d| d.split('.').next()?.parse().ok())
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lokalsi_hwprobe::{CpuInfo, GpuInfo};

    fn profile(gpus: Vec<GpuInfo>, backends: Vec<Backend>) -> HardwareProfile {
        HardwareProfile {
            os: "windows".into(),
            os_version: None,
            arch: "x86_64".into(),
            cpu: CpuInfo {
                brand: String::new(),
                physical_cores: None,
                logical_cores: 8,
                features: vec![],
                ..Default::default()
            },
            ram_mb: 32_768,
            gpus,
            disks: vec![],
            backends,
            probed_at: 0,
            ..Default::default()
        }
    }

    fn nvidia(driver: &str) -> GpuInfo {
        GpuInfo {
            vendor: GpuVendor::Nvidia,
            name: "RTX 3090".into(),
            vram_mb: 24_576,
            driver: Some(driver.into()),
            unified: false,
            source: "nvml".into(),
            ..Default::default()
        }
    }

    #[test]
    fn manifest_is_valid_and_synced() {
        let m = EngineManifest::llama_cpp();
        assert_eq!(m.id, "llama-cpp");
        assert!(
            m.unsynced_files().is_empty(),
            "run `cargo run -p catalog-sync`: {:?}",
            m.unsynced_files()
        );
        assert!(
            m.asset_url(&m.builds[0].files[0])
                .starts_with("https://github.com/ggml-org/llama.cpp/releases/download/")
        );
    }

    #[test]
    fn picks_cuda13_for_new_drivers_and_cuda12_for_old() {
        let m = EngineManifest::llama_cpp();
        let backends = vec![Backend::Cuda, Backend::Vulkan, Backend::Cpu];
        let new = profile(vec![nvidia("610.88")], backends.clone());
        assert_eq!(m.builds_for("windows-x64", Some(&new))[0].id, "win-cuda13");
        let old = profile(vec![nvidia("551.23")], backends);
        let ids: Vec<_> = m
            .builds_for("windows-x64", Some(&old))
            .iter()
            .map(|b| b.id.as_str())
            .collect();
        assert_eq!(ids, ["win-cuda12", "win-vulkan", "win-cpu"]);
    }

    #[test]
    fn linux_builds_follow_the_same_rules() {
        let m = EngineManifest::llama_cpp();
        let new = profile(
            vec![nvidia("590.48")],
            vec![Backend::Cuda, Backend::Vulkan, Backend::Cpu],
        );
        let ids: Vec<_> = m
            .builds_for("linux-x64", Some(&new))
            .iter()
            .map(|b| b.id.as_str())
            .collect();
        assert_eq!(
            ids,
            ["linux-cuda13", "linux-cuda12", "linux-vulkan", "linux-cpu"]
        );
        let amd = profile(vec![], vec![Backend::Vulkan, Backend::Cpu]);
        assert_eq!(m.builds_for("linux-x64", Some(&amd))[0].id, "linux-vulkan");
    }

    #[test]
    fn builtin_mcp_grafana_is_synced_for_every_desktop() {
        let m = EngineManifest::builtin("mcp-grafana").unwrap();
        assert!(m.unsynced_files().is_empty(), "{:?}", m.unsynced_files());
        for platform in ["windows-x64", "macos-arm64", "linux-x64"] {
            assert!(!m.builds_for(platform, None).is_empty(), "{platform}");
        }
        assert!(EngineManifest::builtin("nope").is_none());
    }

    #[test]
    fn whisper_and_sd_manifests_are_synced() {
        for m in [EngineManifest::whisper_cpp(), EngineManifest::sd_cpp()] {
            assert!(
                m.unsynced_files().is_empty(),
                "run `cargo run -p catalog-sync`: {:?}",
                m.unsynced_files()
            );
        }
        assert_eq!(EngineManifest::whisper_cpp().launch.cmd, "whisper-cli");
        assert_eq!(EngineManifest::sd_cpp().launch.cmd, "sd-cli");
    }

    #[test]
    fn whisper_picks_cuda_by_driver_and_cpu_otherwise() {
        let m = EngineManifest::whisper_cpp();
        let host = HostInfo::default();
        let backends = vec![Backend::Cuda, Backend::Vulkan, Backend::Cpu];
        let new = profile(vec![nvidia("610.88")], backends.clone());
        let ids: Vec<_> = m
            .builds_for_host("windows-x64", Some(&new), &host)
            .iter()
            .map(|b| b.id.as_str())
            .collect();
        assert_eq!(ids, ["win-cuda12", "win-cuda11", "win-cpu"]);
        let old = profile(vec![nvidia("472.12")], backends);
        assert_eq!(
            m.builds_for_host("windows-x64", Some(&old), &host)[0].id,
            "win-cuda11"
        );
        // No macOS build of whisper.cpp upstream (ADR 0015).
        assert!(m.builds_for_host("macos-arm64", None, &host).is_empty());
    }

    #[test]
    fn sd_builds_respect_glibc_and_macos_versions() {
        let m = EngineManifest::sd_cpp();
        let amd = profile(vec![], vec![Backend::Vulkan, Backend::Cpu]);
        let noble = HostInfo {
            glibc: Some((2, 39)),
            macos: None,
        };
        assert!(
            m.builds_for_host("linux-x64", Some(&amd), &noble)
                .is_empty()
        );
        let resolute = HostInfo {
            glibc: Some((2, 43)),
            macos: None,
        };
        assert_eq!(
            m.builds_for_host("linux-x64", Some(&amd), &resolute)[0].id,
            "linux-vulkan"
        );
        let mac = profile(vec![], vec![Backend::Metal, Backend::Cpu]);
        let sequoia = HostInfo {
            glibc: None,
            macos: Some(15),
        };
        assert!(
            m.builds_for_host("macos-arm64", Some(&mac), &sequoia)
                .is_empty()
        );
        let tahoe = HostInfo {
            glibc: None,
            macos: Some(26),
        };
        assert_eq!(
            m.builds_for_host("macos-arm64", Some(&mac), &tahoe)[0].id,
            "macos-metal"
        );
        assert_eq!(parse_version("2.43"), Some((2, 43)));
    }

    #[test]
    fn uv_is_synced_for_every_desktop() {
        let m = EngineManifest::uv();
        assert!(
            m.unsynced_files().is_empty(),
            "run `cargo run -p catalog-sync`: {:?}",
            m.unsynced_files()
        );
        for platform in ["windows-x64", "macos-arm64", "linux-x64"] {
            assert_eq!(m.builds_for(platform, None).len(), 1, "{platform}");
        }
        assert!(
            m.asset_url(&m.builds[0].files[0])
                .starts_with("https://github.com/astral-sh/uv/releases/download/")
        );
    }

    #[test]
    fn agent_programs_are_synced_for_every_desktop() {
        for id in ["node", "goose", "opencode", "droid"] {
            let m = EngineManifest::builtin(id).expect(id);
            assert!(
                m.unsynced_files().is_empty(),
                "{id}: run `cargo run -p catalog-sync`"
            );
            for platform in ["windows-x64", "macos-arm64", "linux-x64"] {
                assert_eq!(m.builds_for(platform, None).len(), 1, "{id} {platform}");
            }
        }
        let node = EngineManifest::builtin("node").unwrap();
        let url = node.asset_url(&node.builds[0].files[0]);
        assert!(url.starts_with("https://nodejs.org/dist/v"), "{url}");
        assert!(url.ends_with("-win-x64.zip"), "{url}");
        let droid = EngineManifest::builtin("droid").unwrap();
        let url = droid.asset_url(&droid.builds[0].files[0]);
        assert!(url.ends_with("/1.13.0/windows/x64/droid.exe"), "{url}");
    }

    #[test]
    fn cpu_only_and_apple() {
        let m = EngineManifest::llama_cpp();
        let cpu = profile(vec![], vec![Backend::Cpu]);
        let ids: Vec<_> = m
            .builds_for("windows-x64", Some(&cpu))
            .iter()
            .map(|b| b.id.as_str())
            .collect();
        assert_eq!(ids, ["win-cpu"]);
        let mac = profile(vec![], vec![Backend::Metal, Backend::Cpu]);
        assert_eq!(m.builds_for("macos-arm64", Some(&mac))[0].id, "macos-metal");
    }
}
