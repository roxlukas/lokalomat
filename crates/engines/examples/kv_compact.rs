//! Compact KV cache on a simulated small card (ADR 0021): loads a GGUF model
//! with the context plan for `BUDGET_MB` (default 7 680 = 8 GB card), asks one
//! question and prints the plan, the answer and the GPU memory in use.
//!
//! cargo run --release -p lokalsi-engines --example kv_compact -- <work dir> <model.gguf> <estimate MB>
//!
//! `ENGINES=<dir>` – engines folder to reuse (default `<work dir>/engines`);
//! `SEED_DB=<app.db>` – database that already knows the installed engine.

use std::path::PathBuf;
use std::sync::Arc;

use lokalsi_core::db::Db;
use lokalsi_engines::llama::{ChatMessage, ChatParams};
use lokalsi_engines::manifest::EngineManifest;
use lokalsi_engines::service::{EngineEvents, EngineService, LoadRequest, ctx_plan};
use tokio_util::sync::CancellationToken;

struct Quiet;
impl EngineEvents for Quiet {}

fn gpu_used_mib() -> String {
    std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.used", "--format=csv,noheader"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_else(|_| "?".into())
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

    let plan = ctx_plan(Some(budget), estimate);
    println!(
        "budget {budget} MB, estimate {estimate} MB → ctx {} kv_compact {}",
        plan.ctx_limit, plan.kv_compact
    );
    println!("GPU before: {}", gpu_used_mib());

    let db = Arc::new(Db::open(&db_path).unwrap());
    let llm = EngineService::new(
        EngineManifest::llama_cpp(),
        db,
        reqwest::Client::new(),
        lokalsi_core::library::platform_key(),
        engines,
        &work.join("logs"),
        Arc::new(Quiet),
    );
    llm.set_profile(Some(lokalsi_hwprobe::probe()));
    let req = LoadRequest {
        model_id: "check".into(),
        variant_id: "check".into(),
        name: model.display().to_string(),
        path: model,
        memory_estimate_mb: estimate,
        supports_tools: false,
        ctx_size: None,
        ctx_limit: plan.ctx_limit,
        kv_compact: plan.kv_compact,
    };
    let messages = [ChatMessage {
        role: "user".into(),
        content: Some("Napisz w Pythonie funkcję, która liczy silnię. Krótko.".into()),
        tool_calls: None,
        tool_call_id: None,
    }];
    let started = std::time::Instant::now();
    let out = llm
        .complete(
            &req,
            &messages,
            &ChatParams {
                temperature: Some(0.2),
                max_tokens: Some(2000),
                ..Default::default()
            },
            &CancellationToken::new(),
            &mut |_| {},
        )
        .await;
    println!("GPU loaded: {}", gpu_used_mib());
    match out {
        Ok(o) => println!(
            "answer in {} s:\n{}",
            started.elapsed().as_secs(),
            o.content
        ),
        Err((code, detail)) => println!("ERROR {code}: {detail}"),
    }
    if let Some(loaded) = llm.status(None).loaded {
        println!("loaded ctx {} kv_compact {}", loaded.ctx, loaded.kv_compact);
    }
    llm.stop_server().await;
}
