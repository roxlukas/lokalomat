# ADR 0022: Music generation – ACE-Step 1.5 as the model for a new "Muzyka" mode

- Status: accepted for 1.0 (owner, 2026-11-16) – new milestone M5d before M6 (PRD §13,
  §15 point 22); the YuE2 question and the listening test are still open
- Date: 2026-11-16
- ADR 0021 is reserved by the compact KV cache (branch `claude/kv-8gb-profile`).

## Context

The owner asked for local music models – songs with vocals, "like Suno". The PRD has
no music mode (§6 lists Czat, Obraz, Transkrypcja, Głos; `modality` is
`llm | image | stt | tts`), so this is a deviation from the PRD and needs a new mode,
a new modality and a new engine.

Every fact below comes from the official model card, repository or the Hugging Face /
GitHub API, checked on 2026-11-16. "not stated" means the official source gives no
figure – nothing is guessed. Secondary blog posts were only used to find candidates.

Criteria as in ADR 0018: vocals and Polish lyrics, licence (commercial use, EU – PRD §11
"Dozwolone w UE"), VRAM on 8–24 GB cards, speed, Windows/macOS/Linux, maintenance.

## Candidates

| Model | Vocals / Polish | Licence (weights) | Size / VRAM | Speed (official) | Platforms | State |
|---|---|---|---|---|---|---|
| **ACE-Step 1.5** (ACE Studio + StepFun) | yes; `pl` is one of 50 codes in `VALID_LANGUAGES` (`acestep/constants.py`); no Polish quality figure | **MIT**, card: output may be used commercially | 2B DiT (turbo/sft/base) + Qwen3 LM 0.6B/1.7B/4B; HF `ACE-Step/Ace-Step1.5` ≈ 10.1 GB (DiT turbo 4.79 GB, LM 1.7B 3.71 GB, Qwen3-Embedding-0.6B 1.19 GB, VAE 0.34 GB); XL turbo 4B ≈ 20 GB. Official table: ≤ 6 GB DiT only (INT8 + CPU offload), 6–8 GB + LM 0.6B, 8–16 GB + LM 1.7B, ≥ 20 GB XL without offload | full song < 10 s on RTX 3090, < 2 s on A100 | Windows CUDA/ROCm, Linux CUDA/XPU, macOS MLX/MPS, CPU listed | active (GitHub push 2026-11-08), uv project, Python 3.11–3.12, REST API |
| YuE2-3B (HKUST / M-A-P) | yes; card lists Chinese and English only | **CC BY-NC 4.0** + creator permission: individuals may monetise songs, **companies need a commercial licence** | 7.26 GB weights + separate VAE; quick start asks for 24 GB, peak 11–14 GiB measured on RTX 4090 | 3.6 min song in 71 s on RTX 4090 | Linux + CUDA; Windows and low-memory paths "experimental" | new (paper Sep 2026); self-reported scores above Suno v5/v6 on its own benchmark |
| YuE v1 | yes; en, zh, yue, ja, ko ("not limited to") | Apache-2.0 | ≤ 24 GB only 2 sections (verse + chorus); a full song needs ≥ 80 GB | 30 s of audio in ≈ 360 s on RTX 4090 | Linux/WSL; Windows only through third-party installers | superseded by YuE2 |
| DiffRhythm 2 (ASLP / Xiaomi) | full songs with lyrics; languages not stated | Apache-2.0 (`ASLP-lab/DiffRhythm2`) | not stated | not stated | Linux script; espeak-ng needed on every OS | last dated release 2025-10-30 |
| LeVo 2 / SongGeneration v2 (Tencent) | songs with vocals | **not confirmed** – HF repo has no model card, licence tag `unknown`; a GGUF port states research use only | 7.34 GB (`v2-medium`) | not stated | – | – |
| MusicGen (Meta AudioCraft, incl. `musicgen-melody`) | instrumental only; melody conditioning from a reference tune | **CC BY-NC 4.0** | small–large | 30 s per pass | PyTorch | no updates since 2024 |
| Stable Audio Open 1.0 / small | **no vocals** ("not able to generate realistic vocals"), small: up to 11 s | Stability AI Community License, gated | 0.5B (small), Arm/LiteRT build | – | CPU, CUDA, Arm | sound effects and loops, not songs |

The ≈ 8 GB model with melody control from the owner's note is MusicGen-melody; its
weights are non-commercial, so it fails the licence criterion.

## Decision (proposed)

1. **New mode "Muzyka"** with `modality: "music"` and job kind `music`, engine id
   `ace-step`. Input: style description, lyrics (optional – empty means instrumental),
   length 10 s – 10 min, optional reference audio (Expert). Output: WAV + MP3 in the
   Gallery, with the "generated locally with AI" mark as in ADR 0018.
2. **Default model: ACE-Step 1.5** – the only candidate that is MIT, accepts Polish
   lyrics, runs on 4–24 GB cards and on all three desktop platforms, and is maintained.
   The plan picks the variant from the free VRAM following the official table:
   DiT turbo only below 6 GB, + LM 0.6B up to 12 GB, + LM 1.7B up to 20 GB, XL turbo
   from 20 GB (Expert, ≈ 20 GB download). Basic mode shows one model and no LM choice.
3. **YuE2 not in the catalogue for now**: its weights need a commercial licence for
   companies, and the catalogue's licence field and "Dozwolone w UE" filter cannot
   express "free for individuals only". It can come back as an Expert entry marked
   `commercial: "no"` if the owner wants it, or once M-A-P publishes other terms.
4. **Excluded**: YuE v1 (80 GB for a full song), LeVo 2 (licence unconfirmed),
   MusicGen (non-commercial, no vocals), Stable Audio Open (no vocals, gated, own
   licence). **DiffRhythm 2** stays a candidate for the listening test once its
   languages and VRAM are known.
5. **Polish listening test before release** on the RTX 3090, the 8 GB profile and the
   Linux CPU machine: same Polish lyrics and style, 2B turbo with LM 0.6B / 1.7B and XL
   turbo; time per song, peak VRAM/RAM, lyric accuracy through whisper large-v3-turbo
   (WER) and the owner's rating. Results go into this ADR and ADR 0016.

## Implementation (M5d)

Changes against the sketch above are marked "differs".

- **Engine**: a third environment `envs/ace-step` (`PyEnv::AceStep`, engine id `ace-step`)
  in `crates/engines/src/python.rs`, installed on the first song like the TTS engines
  (plug and play). PyTorch build: CUDA 12.8 for NVIDIA drivers ≥ 570 (also RTX 50),
  CUDA 12.6 for ≥ 525, else CPU; macOS uses PyPI's MPS wheels.
- **Worker – differs**: no separate `workers/python-music`. ACE-Step is a third adapter
  (`lokalsi_tts/adapters/ace_step.py`) of the existing worker, which already has the
  token, offline mode, NDJSON progress, `/unload` and the VRAM-scheduler contract.
  `/synthesize` takes `caption` (style) next to `text` (lyrics, empty = instrumental) and
  makes one whole song; the worker writes stereo WAV + MP3 (192 kbit/s) with the
  "generated locally with AI" mark. Settings follow ACE-Step's own API server: CPU
  offload below 20 GB of VRAM, the PyTorch backend for the language model (no vLLM).
- **Package**: `ace-step` from the GitHub archive of commit `ca1e85f` (no PyPI release).
  `workers/python-tts/overrides-ace-step.txt` drops what song generation does not import
  (Gradio, Lightning, TensorBoard, PEFT, LyCORIS, ModelScope, nano-vLLM, matplotlib,
  typer, torchcodec – checked with `grep` over the inference path, all imported lazily)
  and pins PyTorch without the `+cu128` suffix so `--torch-backend` can pick the build.
  torchao (only needed for INT8 quantization, which the worker does not use) is dropped
  on Windows, where the PyTorch index has no CUDA wheel for it. Resolution checked for
  Windows (cu128, cu126, CPU), Linux (cu128, CPU) and macOS arm64; the Windows cu128
  environment installs to 6.0 GB and imports with CUDA available.
- **Catalogue – folder trees**: ACE-Step needs its checkpoint folders (`acestep-v15-turbo/`,
  `vae/`, `Qwen3-Embedding-0.6B/`, `acestep-5Hz-lm-*/`), but downloads used to keep only
  the file name. Catalogue files now have an optional `local` path inside the variant
  folder (validated: relative, no `..`, unique per variant); such files never come from
  the Hugging Face cache, because the tree must stay together. ACE-Step's own downloader
  is switched off in the adapter, so the worker never goes online.
- **Model code**: ACE-Step writes its model code (`*.py`) next to the weights on first
  load (the code in the Hugging Face repo is older than the package's). The `.py` files
  are therefore not in the catalogue, and deleting a variant removes its whole folder
  when every file in it was downloaded by the app.
- **Variants** (sizes and sha256 from `catalog-sync`): `turbo-lm1.7b` 10.1 GB
  (recommended), `turbo-lm0.6b` 7.7 GB, `turbo` 6.3 GB (no language model, for small
  cards), `xl-turbo-lm1.7b` 25.3 GB. The weights are stored in fp32 and run in bf16, so
  the size-based memory estimate is on the high side: XL shows 27.5 GB and is offered
  with offloading on a 24 GB card although upstream says it fits from 20 GB. Real peaks
  come from the listening test.
- **Speed (ADR 0016)**: new `WorkKind::Music`, real-time factor; first guess from the
  official claim (≈ 0.145 TFLOP per GB of files per audio second → RTF ≈ 20 on an
  RTX 3090). Levels for a 3-minute song: fast ≤ 30 s, usable ≤ 2 min, slow ≤ 10 min.
- **UI**: Workshop mode "Muzyka" (Alt+5; Łańcuch moves to Alt+6): style in the main field,
  lyrics below, length chips (any, 30 s, 1/2/3 min); Expert: model, lyrics language and
  seed. Results reuse the audio card (waveform, WAV/MP3, "Użyj ponownie") and appear in
  the Gallery under "Muzyka". Music is never part of the starter set in the first-run
  wizard; the summary shows a "Piosenki" line.
- **Lyrics and prompts** are user content: logged only through `loggable_content`.
- **Not in M5d**: phone (no mobile build of ACE-Step), pipelines template
  "Tekst › piosenka", reference audio and covers.

## Measurements

2026-11-16, Windows 11, i5-11400F, 64 GB RAM, the worker started like the app does
(uv-managed Python 3.11, environment from `overrides-ace-step.txt`, CUDA 12.8 build),
`turbo-lm1.7b`, Polish lyrics, 10 s song, seed 42:

| Step | Result |
|---|---|
| Environment (Windows, CUDA 12.8) | 6.0 GB on disk; imports with CUDA available |
| CPU (forced), model load | ≈ 55 s (DiT, VAE, text encoder, LM 1.7B) |
| CPU (forced), 10 s song | ≈ 135 s (RTF ≈ 0.07) – works, too slow to be useful |
| Output | stereo 48 kHz WAV + MP3 with the AI mark |

RTX 3090 24 GB, `crates/engines/examples/m5d_acceptance.rs` (environment installed by
`PythonEngine`, songs through `MediaService` like the Workshop), 60 s songs:

| Step | Result |
|---|---|
| `turbo-lm1.7b`, Polish, first song | 73 s incl. worker start and model load (≈ 44 s) |
| `turbo-lm1.7b`, Polish, next song | 28.5 s (RTF ≈ 2.1); LM ≈ 27 s of it (PyTorch backend), DiT + VAE ≈ 7 s |
| `turbo-lm1.7b`, instrumental | 28 s |
| `turbo-lm1.7b`, peak VRAM | ≈ 14.4 GB above the desktop's use |
| `turbo` (no LM), 60 s song | 19 s incl. load; peak ≈ 8 GB |

Upstream's "under 10 s per song" uses vLLM for the LM; the PyTorch backend is about three
times slower but needs no Triton or flash-attention (not available on Windows). The speed
and memory estimates were recalibrated from these numbers (music: files × 1.35 + 1 GiB;
XL is overestimated because its DiT is stored in fp32).

In the app (owner, 2026-11-16, fresh app home, RTX 3090): `turbo-lm1.7b` downloaded from
the Models view (10.1 GB, every file checked); the first song installed the environment by
itself (73 s, 6.0 GB, CUDA 12.8) without a visit to Engines; a 60 s song with 799
characters of Polish lyrics and no description took 85 s cold (78 s generation, worker
start and model load included). With an empty description the LM picks the style itself –
here it chose disco polo, which the owner found funny. A fuller Polish listening test is
still open. A Python from the Microsoft Store crashed
with an access violation while loading the LM; the app never uses it (uv's own Python
only, ADR 0018), and the same files load fine there.

### 8 GB card (RTX 2070, i7-870, 16 GB RAM + 8 GB page file, Windows 10, driver 560.94)

`m5d_acceptance` over SSH, environment installed by itself with the CUDA 12.6 build
(5.3 GB). Three problems came up, and the fixes are:

1. **NaN on pre-Ampere GPUs.** Turing has no bfloat16, so ACE-Step runs in float16, and the
   lyric encoder overflows there (`encoder.lyric_encoder.layers.7.mlp.down_proj` reaches
   ≈ 50 000 before going to Inf), so every song failed with "NaN or Inf latents". The worker
   now runs the condition encoder (608 M parameters, once per song) in float32 on float16
   GPUs (`keep_float32`). It casts per call, because ACE-Step casts the whole model back
   to its dtype on every CPU↔GPU move.
2. **Running out of RAM with CPU offload on Windows.** Every offload round trip commits
   fresh host memory that is not given back. With LM 0.6B the worker went
   10 → 16 → 20 GB of commit and crashed in `c10.dll` (0xc0000005, Windows event 2004
   "low virtual memory") on the second song. Without offload it stays at ≈ 12 GB, and the
   driver spills the excess VRAM (peak ≈ 9.3 GB reserved) into shared memory at the
   same speed. So the worker no longer offloads on Windows; Linux keeps ACE-Step's rule,
   because it has no such spill.
3. **LM 1.7B does not fit 16 GB of RAM.** It ran out of virtual memory while loading the
   LM, with or without offload. Past VRAM, a music variant now needs its whole estimate in
   usable RAM (`music_fit`), so on this machine LM 1.7B shows as "won't run" and the Models
   view offers `turbo-lm0.6b`.

After the fixes, `turbo-lm0.6b`, 60 s songs, seed 42:

| Step | Result |
|---|---|
| Polish, first song | 464 s, including worker start and model load |
| Polish, next song | 171 s (RTF ≈ 0.35) |
| Instrumental | 192 s |
| Peak VRAM / worker commit | ≈ 8.0 GB (card full, driver spill) / 13.5 GB, steady ≈ 12 GB |

The speed formula gives ≈ 0.32 RTF for this variant on this card (≈ 7.5 TFLOPS, 70 % on
the GPU), so it shows as "slow" (RTF 0.3–1.5). That matches the measured 0.35. The variant
without the LM was not measured on this card.

## Open questions for the owner

- ~~Timing~~ – decided 2026-11-16: music is part of 1.0, as milestone M5d before M6.
- Is a non-commercial YuE2 entry in Expert mode acceptable, or should the catalogue stay
  commercially clean?

## Consequences

- New modality, job kind, Workshop mode, locale keys (pl/en), no DB migration (jobs store
  their parameters as JSON), a third Python environment (6.0 GB with CUDA 12.8 on
  Windows) and 10.1 GB of model files for the recommended variant.
- Generated songs can sound like existing artists; the UI says the user is responsible
  for how they use the music, like the consent statement for voices (PRD §11).
