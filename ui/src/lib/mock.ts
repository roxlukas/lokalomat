/**
 * Browser-only stand-ins for desktop commands (`npm run dev:ui`), so views
 * can be designed without the Tauri shell. Never used inside the app.
 */
import catalog from '../../../catalog/models.json';
import { registeredMocks } from './edition/registry';
import { readLocal, writeLocal } from './storage';
import type {
  DecisionAnswer,
  DecisionForm,
  EnqueueOutcome,
  Job,
  Fit,
  FitReport,
  HardwareView,
  MemoryBudget,
  ModelParams,
  ModelView,
  SpeedReport,
  StarterPick,
  StorageOptions,
  VariantView,
} from './ipc';

export const UNSUPPORTED = Symbol('unsupported');

/** The local server in the preview (ADR 0025). */
export const mockServer: import('./ipc').ServerStatus = {
  enabled: false,
  port: 47822,
  lan: false,
  running: false,
  url: null,
  lan_url: null,
  error: null,
  keys: [],
};

/** Decision models look installed in the preview, so the "Decyzje" mode can be designed. */
const MOCK_INSTALLED = new Set(['basal-2.0-4.5b/q8_0']);
let mockJobId = 1000;

/** A finished decision with made-up probabilities (the first answer is the most likely). */
function mockDecision(form: DecisionForm): Job {
  const answers: DecisionAnswer[] = form.questions.map((q, k) => {
    const labels = q.type === 'yes_no' ? ['yes', 'no'] : q.type === 'choice' ? q.options : q.levels;
    const raw = labels.map((_, i) => 1 / (i + 1 + k) ** 2);
    const sum = raw.reduce((a, b) => a + b, 0);
    const probabilities = labels.map((label, i) => ({ label, p: raw[i]! / sum }));
    const top = probabilities[0]!;
    return {
      id: q.id,
      label: top.label,
      probabilities,
      confidence: Math.max(0, (top.p * labels.length - 1) / (labels.length - 1)),
      ...(q.type === 'score' ? { score: probabilities.reduce((s, p, i) => s + p.p * i, 0) } : {}),
    };
  });
  const now = Math.floor(Date.now() / 1000);
  return {
    id: ++mockJobId,
    kind: 'decision',
    status: 'done',
    progress: 1,
    model_id: 'basal-2.0-4.5b',
    variant_id: 'q8_0',
    model_name: 'Basal 2.0 4.5B Q8_0',
    engine: 'llama-cpp',
    params: { questions: form.questions },
    inputs: { chars: form.state.length },
    outputs: { answers, input_tokens: 300 },
    text: form.state,
    error: null,
    created_at: now,
    started_at: now,
    finished_at: now,
    duration_ms: 420,
  };
}

const GB = 1024 ** 3;

const hardware: HardwareView = {
  profile: {
    os: 'windows',
    os_version: 'Windows 11 (browser preview)',
    arch: 'x86_64',
    cpu: { brand: 'Example CPU', physical_cores: 8, logical_cores: 16, features: ['avx2'] },
    ram_mb: 32768,
    gpus: [{ vendor: 'nvidia', name: 'Example GPU 12 GB', vram_mb: 12288, driver: null, unified: false, source: 'mock' }],
    disks: [],
    backends: ['cuda', 'vulkan', 'cpu'],
    probed_at: 0,
  },
  budget: { kind: 'gpu', main_mb: 11776, offload_mb: 28672, device: 'Example GPU 12 GB' },
};

function fitFor(estimate: number, b: MemoryBudget): FitReport {
  const headroom = b.main_mb ? (b.main_mb - estimate) / b.main_mb : -1;
  let fit: Fit = 'wont_run';
  if (estimate <= b.main_mb) fit = headroom >= 0.15 ? 'fits' : 'tight';
  else if (b.offload_mb > 0 && estimate <= b.main_mb + b.offload_mb) fit = 'offload';
  return { fit, estimate_mb: estimate, budget_mb: b.main_mb, offload_mb: b.offload_mb, headroom };
}

/** Rough stand-in for the backend speed estimate (ADR 0016), derived from the fit. */
function speedFor(fit: FitReport, modality: string): SpeedReport {
  const speed = ({ fits: 'fast', tight: 'usable', offload: 'slow', wont_run: 'too_slow' } as const)[fit.fit];
  const unit = modality === 'llm' ? 'tokens_per_second' : modality === 'image' ? 'seconds_per_image' : 'realtime';
  const values = {
    tokens_per_second: { fast: 40, usable: 10, slow: 3, too_slow: 0 },
    seconds_per_image: { fast: 12, usable: 40, slow: 150, too_slow: 0 },
    realtime: { fast: 20, usable: 4, slow: 0.8, too_slow: 0 },
  } as const;
  const value = values[unit][speed];
  return {
    speed,
    value,
    unit,
    prompt_tokens_per_s: modality === 'llm' ? value * 40 : undefined,
    device: fit.fit === 'offload' ? 'mixed' : 'gpu',
    reason: fit.fit === 'wont_run' ? 'not_enough_memory' : undefined,
  };
}

/** Like the desktop app: the free edition offers only chat and decision models (ADR 0035). */
const FREE_MODALITIES = ['llm', 'decision'];

function models(): ModelView[] {
  const budget = hardware.budget!;
  const free = registeredMocks().length === 0;
  return catalog.models.filter((m) => !free || FREE_MODALITIES.includes(m.modality)).map((m) => ({
    ...(m as unknown as Omit<ModelView, 'custom' | 'variants' | 'default_variant' | 'scores'>),
    custom: false,
    default_variant: null,
    // the benchmark is fetched by the desktop app; the browser preview behaves as if the site were down
    scores: null,
    variants: m.variants.map(
      (v): VariantView => ({
        id: v.id,
        label: v.label,
        recommended: 'recommended' in v ? Boolean(v.recommended) : false,
        size: v.files.reduce((s, f) => s + f.size, 0),
        memory_estimate_mb: v.memory_estimate_mb,
        files: v.files.length,
        fit: fitFor(v.memory_estimate_mb, budget),
        speed: speedFor(fitFor(v.memory_estimate_mb, budget), m.modality),
        supported_here: v.platforms.includes('windows-x64'),
        state: MOCK_INSTALLED.has(`${m.id}/${v.id}`) ? 'installed' : 'not_installed',
        downloaded: 0,
      }),
    ),
  }));
}

function starterPack(): StarterPick[] {
  const picks: StarterPick[] = [];
  for (const modality of ['llm', 'image', 'stt', 'tts'] as const) {
    const m = models().find((x) => x.modality === modality && x.variants.some((v) => v.fit?.fit === 'fits'));
    const v = m?.variants.find((x) => x.recommended) ?? m?.variants[0];
    if (m && v)
      picks.push({
        modality,
        model_id: m.id,
        model_name: m.name,
        variant_id: v.id,
        variant_label: v.label,
        size: v.size,
        fit: v.fit,
        speed: v.speed,
        installed: false,
      });
  }
  return picks;
}

const storage: StorageOptions = {
  current: null,
  suggested: 'C:\\Users\\you\\AppData\\Local\\Studio Lokalsi\\data',
  disks: [
    { name: 'System', mount_point: 'C:\\', total_bytes: 500 * GB, available_bytes: 60 * GB, kind: 'ssd', removable: false, suggested_dir: 'C:\\Users\\you\\AppData\\Local\\Studio Lokalsi\\data' },
    { name: 'Data', mount_point: 'D:\\', total_bytes: 2000 * GB, available_bytes: 900 * GB, kind: 'hdd', removable: false, suggested_dir: 'D:\\Studio Lokalsi' },
  ],
};

const MODEL_PARAMS_KEY = 'lokalsi.mock-model-params';

function mockModelParams(): Record<string, ModelParams> {
  return readLocal<Record<string, ModelParams>>(MODEL_PARAMS_KEY, {});
}

export async function mockCall(command: string, args?: Record<string, unknown>): Promise<unknown> {
  // The edition's own commands (Pro) first.
  for (const handler of registeredMocks()) {
    const result = await handler(command, args);
    if (result !== UNSUPPORTED) return result;
  }
  switch (command) {
    case 'get_edition':
      return 'free';
    case 'get_hardware':
      await new Promise((r) => setTimeout(r, 600));
      return hardware;
    case 'get_storage_options':
      return storage;
    case 'list_models':
      return models();
    case 'get_starter_pack':
      return starterPack();
    case 'list_downloads':
      return [];
    case 'download_variant': {
      const m = models().find((x) => x.id === args?.modelId);
      const v = m?.variants.find((x) => x.id === args?.variantId);
      return { kind: 'queued', files: v?.files ?? 0, bytes: v?.size ?? 0, reused: 0 } satisfies EnqueueOutcome;
    }
    case 'get_hf_token_status':
      return false;
    case 'get_benchmark_meta':
      return null;
    case 'get_engines':
      return [
        {
          id: 'llama-cpp',
          name: 'llama.cpp',
          version: 'v0.9.0',
          capabilities: ['chat'],
          license: 'MIT',
          state: 'not_installed',
          installed: null,
          update_available: false,
          builds: [{ id: 'win-cuda13', backend: 'cuda', size: 592711011, recommended: true }],
          install_progress: null,
          loaded: null,
          error: null,
          log_path: '',
        },
        ...(['whisper-cpp', 'sd-cpp'] as const).map((id) => ({
          id,
          name: id === 'whisper-cpp' ? 'whisper.cpp' : 'stable-diffusion.cpp',
          version: id === 'whisper-cpp' ? 'v1.9.5' : 'master-1088-32d516c',
          capabilities: [id === 'whisper-cpp' ? 'stt' : 'image'],
          license: 'MIT',
          state: 'not_installed',
          installed: null,
          update_available: false,
          builds: [{ id: 'win-cuda12', backend: 'cuda', size: 600_000_000, recommended: true }],
          install_progress: null,
          loaded: null,
          error: null,
          log_path: '',
        })),
        ...(['piper', 'chatterbox'] as const).map((id) => ({
          id,
          name: id === 'piper' ? 'Piper' : 'Chatterbox',
          version: '0.1.0',
          capabilities: id === 'piper' ? ['tts'] : ['tts', 'voice_cloning'],
          license: id === 'piper' ? 'GPL-3.0' : 'MIT',
          state: 'not_installed',
          installed: null,
          update_available: false,
          builds: [{ id: id === 'piper' ? 'default' : 'cu126', backend: id === 'piper' ? 'cpu' : 'cuda', size: id === 'piper' ? 125_829_120 : 4_294_967_296, recommended: true }],
          install_progress: null,
          loaded: null,
          error: null,
          log_path: '',
          kind: 'python',
          stage: null,
          disk_size: null,
        })),
        {
          id: 'ace-step',
          name: 'ACE-Step',
          version: '0.1.0',
          capabilities: ['music'],
          license: 'MIT',
          state: 'not_installed',
          installed: null,
          update_available: false,
          builds: [{ id: 'cu128', backend: 'cuda', size: 6_442_450_944, recommended: true }],
          install_progress: null,
          loaded: null,
          error: null,
          log_path: '',
          kind: 'python',
          stage: null,
          disk_size: null,
        },
      ];
    case 'list_sessions':
    case 'list_jobs':
      return [];
    case 'get_memory':
      return { budget_mb: hardware.budget?.main_mb ?? null, used_mb: 0, keep_llm: false, items: [] };
    case 'ensure_engines':
      return null;
    case 'decide':
      return mockDecision(args?.form as DecisionForm);
    case 'check_update':
      return null;
    case 'get_server':
      return mockServer;
    case 'set_server':
      mockServer.enabled = Boolean(args?.enabled);
      if (typeof args?.lan === 'boolean') mockServer.lan = args.lan;
      mockServer.running = mockServer.enabled;
      mockServer.url = mockServer.enabled ? `http://127.0.0.1:${mockServer.port}/v1` : null;
      mockServer.lan_url = mockServer.enabled && mockServer.lan ? `http://192.168.1.250:${mockServer.port}/v1` : null;
      return mockServer;
    case 'create_api_key': {
      const id = Math.random().toString(16).slice(2, 14);
      const key = { id, name: String(args?.name ?? ''), agent: null, created_at: Math.floor(Date.now() / 1000), last_used_at: null, requests: 0 };
      mockServer.keys = [key, ...mockServer.keys];
      return { key, secret: `lk-${id}.${'0'.repeat(48)}` };
    }
    case 'api_key_secret':
      return `lk-${String(args?.id)}.${'0'.repeat(48)}`;
    case 'revoke_api_key':
      mockServer.keys = mockServer.keys.filter((k) => k.id !== args?.id);
      return mockServer;
    case 'server_log':
      return mockServer.running
        ? [{ at: Math.floor(Date.now() / 1000), key: 'Goose', method: 'POST', path: '/v1/chat/completions', model: 'qwen3.5-4b:q4_k_m', status: 200, duration_ms: 812, stream: true }]
        : [];
    case 'get_model_params':
      return mockModelParams()[`${String(args?.modelId)}/${String(args?.variantId)}`] ?? { temperature: null, top_p: null, top_k: null, gpu_layers: null };
    case 'get_model_layers':
      return 36;
    case 'set_model_params': {
      const all = mockModelParams();
      all[`${String(args?.modelId)}/${String(args?.variantId)}`] = args?.params as ModelParams;
      writeLocal(MODEL_PARAMS_KEY, all);
      return args?.params;
    }
    case 'get_network':
      return { enabled: false, port: 47821, running: false, address: null, error: null, devices: [], pairing: null };
    default:
      return UNSUPPORTED;
  }
}
