# ADR 0018: TTS models and the Python worker for M4 – Piper by default, Chatterbox Multilingual V3 for cloning

- Status: accepted (owner, 2026-11-12); the Polish listening test follows later
- Date: 2026-11-12

## Context

M4 needs a TTS model with Polish and voice cloning (PRD §13, §14.7) and a light TTS
without cloning that runs on CPU (PRD §8). Model names in the PRD are examples; this ADR
checks what is current. Every fact below comes from the official model card, repository or
Hugging Face/PyPI/GitHub API, checked on 2026-11-12. "not stated" means the official
source gives no figure – nothing is guessed.

Criteria: Polish quality, licence (commercial use, EU – PRD §11 "Dozwolone w UE"), VRAM/RAM,
speed on GPU and CPU, consent safeguards for cloning.

## Candidates with Polish and voice cloning

| Model | Polish | Licence (weights) | Size / VRAM | Speed (official) | CPU | Safeguards | State |
|---|---|---|---|---|---|---|---|
| **Chatterbox Multilingual V3** (Resemble AI) | yes, 1 of 23 languages; no Polish eval, no Polish language pack | **MIT** | 0.5B; files ≈ 3.2 GB (`t3_mtl23ls_v3` 2.14 GB + `s3gen_v3` 1.06 GB + `ve` 6 MB); VRAM not stated | not stated for Multilingual | `device="cpu"/"mps"` supported in code, speed not stated | **Perth watermark in every output**, survives MP3; no consent mechanism | repo says "retired research, no longer maintained" since Aug 2026; `chatterbox-tts` 0.1.7 pins torch 2.6.0, transformers 5.2.0 |
| **VoxCPM2** (OpenBMB) | yes, 1 of 30 languages; card: "Performance varies across languages depending on training data availability" | **Apache-2.0**, "free for commercial use" | 2B; files ≈ 4.96 GB; **≈ 8 GB VRAM** | RTF ≈ 0.30 on RTX 4090 (0.13 with Nano-vLLM) | not stated | asks to label AI audio; no watermark | active (HF updated 2026-11-09); 48 kHz output; torch ≥ 2.5, CUDA ≥ 12 |
| OmniVoice (k2-fsa) | yes, 911 h Polish training data (of 600+ languages) | **CC-BY-NC** (Emilia data) | 0.6B; VRAM not stated | RTF 0.09 on H100 | not stated | disclaimer only | active |
| XTTS-v2 (Coqui) | yes, 1 of 17 | **CPML – non-commercial only** | not stated | not stated | yes | none | Coqui closed; community fork |
| Fish Audio S2 Pro | listed under "other languages" (no tier) | **Fish Audio Research License** – commercial needs separate licence | 5B | H200 figures only | no | none | active |
| F5-TTS | no official Polish | CC-BY-NC-4.0 | – | – | – | – | – |

Checked and **without Polish**: Qwen3-TTS (10 languages), Voxtral TTS (9 languages,
weights CC BY-NC), Chatterbox Turbo / Nano / Flash (English only), VibeVoice (en, zh),
Kokoro-82M (en in the HF card).

No official source publishes a Polish quality score for any of these models.

## Light TTS without cloning: Piper

- Engine: `piper-tts` 1.8.0 on PyPI (`OHF-Voice/piper1-gpl`), **GPL-3.0-or-later**,
  embeds espeak-ng; wheels for Windows x64, macOS x64/arm64, Linux x64/arm64 (abi3).
  It runs in its own process and environment, never linked into the app, so the app's
  licence is not affected. The old MIT `rhasspy/piper` binary is no longer developed.
- Polish voices in `rhasspy/piper-voices` and the data behind them (model cards checked
  2026-11-12): `darkman` (medium, male) – CC0 data, fine-tuned from the CC0 `rdh` voice:
  **commercially clean**. `gosia`, `mc_speech` (medium) and `bass` (high) are fine-tuned
  from `en_US-lessac`, whose Blizzard 2013 licence forbids commercial use and voice
  synthesis products; `mls_6892` (low) comes from `en_US-ryan` (CC BY-NC-SA 4.0). These
  four stay out of the catalogue, so there is no female Piper voice for now.
- ONNX, ≈ 63 MB per medium voice, CPU only is enough.

## Decision

1. **Default TTS: Piper** with `darkman`, in Basic mode the only choice on computers without
   a GPU. No PyTorch in its environment, so it installs fast.
2. **Default cloning model: Chatterbox Multilingual V3** – the only candidate that is both
   MIT/Apache, Polish, small enough for 6–8 GB GPUs and CPU-capable, and it adds a
   watermark to every file, which fits PRD §11. Risk: no longer maintained, so the worker
   pins exact versions and the catalogue keeps the V2 variant as a fallback.
3. **VoxCPM2 later**: planned as a second cloning model for GPUs with ≥ 8 GB free
   (Apache-2.0, maintained, 48 kHz), but not part of M4 – its adapter, environment and
   catalogue entry come with the listening test, so no model ships untested.
4. **Excluded** from the catalogue: OmniVoice, XTTS-v2, Fish S2, F5-TTS, Voxtral (non-commercial
   licences), Qwen3-TTS and English-only models (no Polish), and the Piper voices above.
5. **Later (owner, 2026-11-12: after M4 is built): a Polish listening test** on the RTX 3090
   and on the Linux CPU machine – same Polish text, same 20 s consented reference,
   Chatterbox V3 vs V2 vs VoxCPM2: RTF, peak VRAM/RAM, CER through whisper large-v3-turbo,
   and the owner's subjective rating. Results go into this ADR and into ADR 0016.

Consent safeguards are ours, not the models': the mandatory statement in V6 (PRD §11), the
statement stored with the voice profile (and in exported voices; importing asks again),
and the "generated locally with AI" mark in the files (WAV `LIST/INFO`, MP3 ID3 `TSSE`/`COMM`).
Chatterbox's Perth watermark is kept on; the install check fails without it.

## Implementation (M4)

- **`uv`** is an ordinary engine manifest (`catalog/engines/uv.json`, 0.13.7, six platforms):
  downloaded from the official release, sha256 from the GitHub digests via `catalog-sync`.
- **`crates/engines/src/python.rs`** owns `<engines>/python/`: a private Python 3.11
  (`UV_PYTHON_PREFERENCE=only-managed`, never the system one), the uv cache, the worker
  sources (embedded in the binary, written to `worker-<version>-<hash>/`) and one
  environment per engine – `envs/piper` and `envs/chatterbox`, the two engine ids of the
  catalogue. `uv pip install "<src>[<engine>]" --overrides overrides.txt` installs it;
  for Chatterbox `--torch-backend` is `cu126` for NVIDIA drivers ≥ 525, `cu118` ≥ 452, else
  `cpu`; macOS uses PyPI's MPS wheels. Progress comes from uv's `Downloading …` lines.
  A check imports the packages and records the device PyTorch sees (`cuda`/`metal`/`cpu`).
  When the embedded sources change, the next job updates the environment (plug and play).
- **Pins** (`workers/python-tts/pyproject.toml`): Chatterbox V3 is only on GitHub (PyPI 0.1.7
  predates it), so the package comes from the commit archive `ab55923`; its git dependency on
  Perth is overridden with `resemble-perth==1.0.1` from PyPI (no git client needed); Perth
  needs `pkg_resources`, so `setuptools==80.9.0` is pinned; `torch==2.6.0` as upstream tested.
- **Worker** (`workers/python-tts`, FastAPI + uvicorn on 127.0.0.1, bearer token, offline:
  `HF_HUB_OFFLINE=1`): `/health`, `/synthesize` (NDJSON progress per piece of text, WAV +
  MP3 via `lameenc`), `/unload`. Long texts are split into sentences (≤ 220 characters for
  Chatterbox, which stops after ~40 s per call). The model stays loaded and is a resident of
  the VRAM scheduler (ADR 0015), which can unload it like the chat model. Text and audio
  are never logged, only lengths.
- **"Napraw środowisko"** (Engines view, Expert): removes the environment and installs it
  with `--reinstall --refresh`.
- **Voice library**: migration 6 `voices` table; recordings as mono WAV in
  `<data_dir>/voices/`; quality indicator from 50 ms frames (speech vs noise floor, clipping,
  loudness, 10–30 s length); export/import as a ZIP with `voice.json` + `sample.wav`.

## Measurements (2026-11-12, `crates/engines/examples/m4_acceptance.rs`)

RTX 3090 24 GB, i5-11400F, Windows 11, empty data folder (uv, Python, PyTorch all fresh):

| Step | Result |
|---|---|
| Install Piper environment | 28 s, 155 MB on disk |
| Install Chatterbox environment (CUDA 12.6) | 320 s, 5.0 GB on disk |
| Voice from the first 20 s of a recording | quality "good" |
| Piper `darkman`, Polish, 14.2 s of speech | 5.6 s cold (incl. worker start); warm 8.3 s in 0.36 s (≈ 23×) |
| Chatterbox V3, cloned voice, 12.2 s of speech | 65 s cold (worker start + model load), warm 12.0 s in 11.5 s (≈ 1.05×) |
| Chatterbox V3 on the CPU (forced), 4.4 s of speech | first load 337 s, warm 25 s (≈ 0.17×) |
| whisper large-v3-turbo on the cloned speech | WER 10 % vs the text (only names and one word misheard) |
| "Napraw środowisko" for Piper | 8 s |

Linux test machine: Ubuntu 24.04, i5-7300U (2 cores), 16 GB, no dedicated GPU, empty folder:

| Step | Result |
|---|---|
| Install Piper / Chatterbox (PyTorch CPU) | 5 s, 185 MB / 29 s, 1.5 GB (fast network) |
| Piper, Polish | warm 8.3 s of speech in 0.6 s (≈ 14×); cold 5.2 s in 3.2 s |
| Chatterbox V3, cloned voice, 5.2 s of speech | 115 s cold, 74 s warm (≈ 0.07×) – works, too slow to be useful |
| whisper large-v3-turbo on the cloned speech | WER 0 % |
| "Napraw środowisko" for Piper | 2 s |

Speed levels (ADR 0016) were recalibrated from these numbers: Piper is always estimated
on the CPU; Chatterbox on a GPU is latency-bound (≈ 0.74 s per audio second of token loop
plus bandwidth and vocoder), not bandwidth-bound as first assumed. Narration levels:
≥ 2× fast, ≥ 0.8× usable, ≥ 0.25× slow, below that too slow.

## Known gaps

- **RTX 50 series**: torch 2.6 has no Blackwell (sm_120) kernels; Chatterbox would fail there
  until the pin moves to torch ≥ 2.7 with CUDA 12.8 (needs testing on such a card).
- **AMD/Intel GPUs** run Chatterbox on the CPU (no ROCm/XPU wheel chosen yet).
- **Linux microphone**: WebKitGTK keeps `getUserMedia` off by default; the recorder then
  says so and the user picks a recording file instead.
- **macOS** is not verified (no CI runner); the microphone text is in `Info.plist`.

## Consequences

- Two Python environments created by `uv` on demand: `piper` (small, onnxruntime) and
  `chatterbox` (PyTorch with CUDA/MPS/CPU, ≈ 5 GB with CUDA).
- Catalogue: `chatterbox-multilingual` gets a recommended `v3` variant and keeps `v2`.
  Sizes and sha256 only via `catalog-sync`.
