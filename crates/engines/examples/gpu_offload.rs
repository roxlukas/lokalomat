//! Acceptance of the Inspector's model parameters on real hardware (ADR 0036):
//! GPU offload (`-ngl`) and sampling defaults reach `llama-server`.
//!
//! cargo run --release -p lokalsi-engines --example gpu_offload -- <work dir> <model.gguf> <estimate MB>
//!
//! For each GPU setting (Auto, processor only, half of the layers, all) the model
//! is loaded again, and the example prints the GPU memory in use and the speed.
//! The llama.cpp build no longer logs how many layers it offloaded, so the
//! check is the GPU memory (nvidia-smi): processor only < half < all layers.
//! Then it checks that temperature 0 repeats the same answer and a high
//! temperature does not. Reasoning models answer in `reasoning` first, so the
//! text compared is reasoning and answer together. The exit code is 1 when a
//! check fails.
//!
//! `ENGINES=<dir>` – engines folder to reuse (default `<work dir>/engines`);
//! `SEED_DB=<app.db>` – database that already knows the installed engine;
//! `BUDGET_MB` – context plan budget (default 7 680 = an 8 GB card);
//! `TOKENS` – answer length (default 96).

use std::path::PathBuf;
use std::sync::Arc;

use lokalsi_core::db::Db;
use lokalsi_core::gguf;
use lokalsi_core::model_params::{GPU_LAYERS_ALL, ModelParams, ModelSettings};
use lokalsi_engines::llama::{ChatMessage, ChatOutcome, ChatParams};
use lokalsi_engines::manifest::EngineManifest;
use lokalsi_engines::service::{EngineEvents, EngineService, LoadRequest, ctx_plan};
use tokio_util::sync::CancellationToken;

struct Quiet;
impl EngineEvents for Quiet {}

fn gpu_used_mib() -> Option<u64> {
    let out = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.used", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()?
        .trim()
        .parse()
        .ok()
}

/// Everything the model wrote: a reasoning model answers in `reasoning` first.
fn written(o: &ChatOutcome) -> String {
    format!("{}\n{}", o.reasoning.trim(), o.content.trim())
        .trim()
        .to_owned()
}

struct Checks {
    failed: u32,
}

impl Checks {
    fn check(&mut self, ok: bool, what: impl std::fmt::Display) {
        println!("  [{}] {what}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            self.failed += 1;
        }
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let work = PathBuf::from(&args[1]);
    let model = PathBuf::from(&args[2]);
    let estimate: u64 = args[3].parse().expect("estimate in MB");
    let budget: u64 = std::env::var("BUDGET_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7_680);
    let tokens: u32 = std::env::var("TOKENS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(96);
    std::fs::create_dir_all(&work).unwrap();
    let db_path = work.join("app.db");
    if let Ok(seed) = std::env::var("SEED_DB")
        && !db_path.exists()
    {
        std::fs::copy(seed, &db_path).unwrap();
    }
    let engines = std::env::var("ENGINES")
        .map(PathBuf::from)
        .unwrap_or_else(|_| work.join("engines"));

    let layers = gguf::block_count(&model)
        .expect("readable GGUF")
        .expect("the model file has a layer count");
    println!("model {} has {layers} layers", model.display());

    let plan = ctx_plan(Some(budget), estimate);
    let db = Arc::new(Db::open(&db_path).unwrap());
    let logs = work.join("logs");
    let llm = EngineService::new(
        EngineManifest::llama_cpp(),
        Arc::clone(&db),
        reqwest::Client::new(),
        lokalsi_core::library::platform_key(),
        engines,
        &logs,
        Arc::new(Quiet),
    );
    llm.set_profile(Some(lokalsi_hwprobe::probe()));
    let req = LoadRequest {
        model_id: "check".into(),
        variant_id: "check".into(),
        name: model.display().to_string(),
        path: model.clone(),
        memory_estimate_mb: estimate,
        supports_tools: false,
        ctx_size: None,
        ctx_limit: plan.ctx_limit,
        kv_compact: plan.kv_compact,
    };
    let messages = [ChatMessage::text(
        "user",
        "Wymień pięć owoców, po jednym w linii.",
    )];
    let store = ModelSettings::new(&db);
    let cancel = CancellationToken::new();
    let mut checks = Checks { failed: 0 };

    // The request itself never sets sampling values: they come from the model's defaults.
    let ask = |max_tokens: u32| ChatParams {
        max_tokens: Some(max_tokens),
        ..Default::default()
    };

    println!("\nGPU memory before: {:?} MiB\n", gpu_used_mib());
    let cases: [(&str, Option<u32>); 4] = [
        ("Auto", None),
        ("processor only", Some(0)),
        ("half of the layers", Some(layers / 2)),
        ("all layers", Some(GPU_LAYERS_ALL)),
    ];
    let mut gpu_mem = Vec::new();
    for (name, gpu_layers) in cases {
        println!("== GPU layers: {name} ({gpu_layers:?})");
        store
            .set(
                "check",
                "check",
                &ModelParams {
                    temperature: Some(0.0),
                    gpu_layers,
                    ..Default::default()
                },
            )
            .unwrap();
        let started = std::time::Instant::now();
        let out = llm
            .complete(&req, &messages, &ask(tokens), &cancel, &mut |_| {})
            .await;
        let out = match out {
            Ok(o) => o,
            Err((code, detail)) => {
                checks.check(false, format!("answer: {code}: {detail}"));
                continue;
            }
        };
        let mem = gpu_used_mib();
        println!(
            "  answered in {:.1} s, {:?} tok/s, GPU memory {mem:?} MiB",
            started.elapsed().as_secs_f32(),
            out.tokens_per_second.map(|v| (v * 10.0).round() / 10.0),
        );
        checks.check(!written(&out).is_empty(), "the model wrote something");
        gpu_mem.push((name, mem));
    }

    // More layers on the card use more GPU memory: processor only < half < all.
    let mem_of = |case: &str| {
        gpu_mem
            .iter()
            .find(|(n, _)| *n == case)
            .and_then(|(_, m)| *m)
    };
    if let (Some(cpu), Some(half), Some(all)) = (
        mem_of("processor only"),
        mem_of("half of the layers"),
        mem_of("all layers"),
    ) {
        checks.check(
            cpu < half && half < all,
            format!("GPU memory grows with the layers: {cpu} < {half} < {all} MiB"),
        );
    } else {
        checks.check(
            false,
            "GPU memory could not be read (is nvidia-smi on the PATH?)",
        );
    }

    // Sampling defaults: temperature 0 repeats, a high temperature does not.
    println!("\n== Sampling defaults of the model");
    let answers = |p: ModelParams, n: usize| {
        let store = &store;
        let llm = &llm;
        let (req, messages, cancel, ask) = (&req, &messages, &cancel, &ask);
        async move {
            store.set("check", "check", &p).unwrap();
            let mut all = Vec::new();
            for _ in 0..n {
                let o = llm
                    .complete(req, messages, &ask(tokens), cancel, &mut |_| {})
                    .await
                    .map(|o| written(&o))
                    .unwrap_or_default();
                all.push(o);
            }
            all
        }
    };
    let cold = answers(
        ModelParams {
            temperature: Some(0.0),
            gpu_layers: Some(GPU_LAYERS_ALL),
            ..Default::default()
        },
        2,
    )
    .await;
    checks.check(
        cold.len() == 2 && !cold[0].is_empty() && cold[0] == cold[1],
        "temperature 0: two answers are identical",
    );
    let hot = answers(
        ModelParams {
            temperature: Some(1.8),
            top_k: Some(0),
            gpu_layers: Some(GPU_LAYERS_ALL),
            ..Default::default()
        },
        3,
    )
    .await;
    let distinct = hot.iter().collect::<std::collections::HashSet<_>>().len();
    checks.check(
        distinct > 1,
        format!("temperature 1.8, top_k off: {distinct} different answers out of 3"),
    );
    let one = answers(
        ModelParams {
            temperature: Some(1.8),
            top_k: Some(1),
            gpu_layers: Some(GPU_LAYERS_ALL),
            ..Default::default()
        },
        3,
    )
    .await;
    let distinct = one.iter().collect::<std::collections::HashSet<_>>().len();
    checks.check(
        distinct == 1,
        format!("top_k 1 makes any temperature repeat ({distinct} different answers out of 3)"),
    );

    llm.stop_server().await;
    println!("\n{} check(s) failed", checks.failed);
    if checks.failed > 0 {
        std::process::exit(1);
    }
}
