/**
 * Typed wrappers for Tauri IPC commands and events. Outside Tauri (plain
 * `npm run dev:ui` in a browser) a small fallback keeps the UI usable for
 * layout work (see ./mock.ts).
 */
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { readLocal, writeLocal } from './storage';

export type Language = 'pl' | 'en';
export type Theme = 'system' | 'light' | 'dark';
export type LogMode = 'normal' | 'verbose';
export type LogLevel = 'error' | 'warn' | 'info' | 'debug';
/** Which key sends a chat prompt (ADR 0036). */
export type SendKey = 'ctrl_enter' | 'enter';

export interface AppSettings {
  language: Language;
  theme: Theme;
  log_mode: LogMode;
  log_prompts: boolean;
  data_dir: string | null;
  onboarded: boolean;
  bandwidth_limit_kbps: number | null;
  unified_memory_percent: number;
  /** Expert mode: show advanced options (Basic hides them). */
  expert: boolean;
  /** Context window in tokens; null = automatic (fits free GPU memory). */
  context_size: number | null;
  /** Keep the chat model loaded when images or transcription need memory. */
  keep_llm_loaded: boolean;
  /** Share the local API with paired phones in the local network (off by default). */
  lan_enabled: boolean;
  lan_port: number;
  /** Local OpenAI-compatible server (ADR 0025). */
  server_enabled: boolean;
  server_port: number;
  server_lan: boolean;
  /** Check for a new version at startup (ADR 0029). */
  update_check: boolean;
  /** Which key sends a chat prompt (ADR 0036). */
  send_key: SendKey;
}

/** Inspector defaults of a chat model variant (ADR 0036); null = Auto. */
export interface ModelParams {
  temperature: number | null;
  top_p: number | null;
  top_k: number | null;
  /** Layers on the GPU (Expert): 0 = processor only, `GPU_LAYERS_ALL` = all. */
  gpu_layers: number | null;
}

export const AUTO_MODEL_PARAMS: ModelParams = { temperature: null, top_p: null, top_k: null, gpu_layers: null };

/** A newer version found by the updater (ADR 0029). */
export interface UpdateInfo {
  version: string;
  current_version: string;
  notes: string | null;
  /** A Linux .deb install: download the new version from the release page. */
  manual: boolean;
}

export interface SettingsView {
  settings: AppSettings;
  effective_log_mode: LogMode;
  log_mode_overridden: boolean;
}

/** Free open-source Lokalomat or Lokalomat Pro (ADR 0035). */
export type Edition = 'free' | 'pro';

export interface AppInfo {
  version: string;
  os: string;
  arch: string;
  cpu_logical_cores: number;
  home: string;
  log_dir: string;
}

export interface ExportSummary {
  path: string;
  files: number;
  bytes: number;
}

// --- hardware -------------------------------------------------------------

export type GpuVendor = 'nvidia' | 'amd' | 'intel' | 'apple' | 'other';
export type Backend = 'cuda' | 'metal' | 'vulkan' | 'cpu';

export interface GpuInfo {
  vendor: GpuVendor;
  name: string;
  vram_mb: number;
  driver: string | null;
  unified: boolean;
  source: string;
  /** NVIDIA SMs, AMD CUs, Intel EUs/Xe-cores, Apple GPU cores. */
  compute_units?: number;
  shader_cores?: number;
  clock_mhz?: number;
  memory_bandwidth_gbs?: number;
  fp32_tflops?: number;
  integrated?: boolean;
  details_source?: string;
}

export interface DiskInfo {
  name: string;
  mount_point: string;
  total_bytes: number;
  available_bytes: number;
  removable: boolean;
  kind: string;
}

export interface HardwareProfile {
  os: string;
  os_version: string | null;
  arch: string;
  cpu: {
    brand: string;
    physical_cores: number | null;
    logical_cores: number;
    features: string[];
    performance_cores?: number;
    efficiency_cores?: number;
  };
  ram_mb: number;
  gpus: GpuInfo[];
  disks: DiskInfo[];
  backends: Backend[];
  probed_at: number;
}

export type BudgetKind = 'gpu' | 'unified' | 'cpu';

export interface SpeedBasis {
  gpu_bandwidth_gbs: number;
  gpu_tflops: number;
  cpu_bandwidth_gbs: number;
  cpu_tflops: number;
  cpu_stt_tflops?: number;
  accelerator: string | null;
  estimated: boolean;
}

export interface MemoryBudget {
  kind: BudgetKind;
  main_mb: number;
  offload_mb: number;
  device: string | null;
  speed?: SpeedBasis;
}

export interface HardwareView {
  profile: HardwareProfile | null;
  budget: MemoryBudget | null;
}

export interface DiskOption extends Omit<DiskInfo, 'removable'> {
  removable: boolean;
  suggested_dir: string;
}

export interface StorageOptions {
  current: string | null;
  suggested: string;
  disks: DiskOption[];
}

// --- catalog / library -------------------------------------------------------

export type Modality = 'llm' | 'image' | 'stt' | 'tts' | 'music' | 'decision';
export const MODALITIES: Modality[] = ['llm', 'image', 'stt', 'tts', 'music', 'decision'];
/** Modalities shown only in Expert mode (ADR 0024). */
export const EXPERT_MODALITIES: Modality[] = ['decision'];
export type Fit = 'fits' | 'tight' | 'offload' | 'wont_run';

export interface FitReport {
  fit: Fit;
  estimate_mb: number;
  budget_mb: number;
  offload_mb: number;
  headroom: number;
}

/** ADR 0016: green / yellow / orange / red. */
export type Speed = 'fast' | 'usable' | 'slow' | 'too_slow';
export const SPEED_RANK: Record<Speed, number> = { fast: 3, usable: 2, slow: 1, too_slow: 0 };

export interface SpeedReport {
  speed: Speed;
  /** tokens/s (chat), × real time (transcription, narration) or seconds per image. */
  value: number;
  unit: 'tokens_per_second' | 'realtime' | 'seconds_per_image';
  prompt_tokens_per_s?: number;
  device: 'gpu' | 'cpu' | 'mixed';
  reason?: 'not_enough_memory' | 'needs_gpu';
}

export interface License {
  name: string;
  url: string;
  commercial: 'yes' | 'conditions' | 'no';
  conditions_pl?: string;
  conditions_en?: string;
  eu_allowed: boolean;
  gated: boolean;
}

export type VariantState = 'not_installed' | 'queued' | 'downloading' | 'paused' | 'failed' | 'partial' | 'installed';

export interface VariantView {
  id: string;
  label: string;
  recommended: boolean;
  size: number;
  memory_estimate_mb: number;
  files: number;
  fit: FitReport | null;
  /** Estimated speed on this computer; null until the hardware is probed. */
  speed?: SpeedReport | null;
  supported_here: boolean;
  state: VariantState;
  downloaded: number;
}

export interface ModelView {
  id: string;
  name: string;
  publisher: string;
  modality: Modality;
  engine: string;
  /** Parameters in billions (LLMs); MoE models also give the active ones. */
  params_b?: number;
  active_params_b?: number;
  description_pl: string;
  description_en: string;
  languages: string[];
  features: string[];
  license: License;
  tags: string[];
  custom: boolean;
  variants: VariantView[];
  default_variant: string | null;
  /** AI Transformation benchmark scores (1–5), when the model was tested. */
  scores: ModelScores | null;
}

export interface CategoryScore {
  avg: number;
  n: number;
}

export interface ModelScores {
  name: string;
  overall: number;
  rated: number;
  tokens_per_second: number | null;
  quantization: string | null;
  params_b: number | null;
  categories: Record<string, CategoryScore>;
}

export interface BenchmarkMeta {
  title: string;
  source_url: string;
  data_url: string;
  license: string;
  attribution: string;
  generated_at: string;
  evaluator: string;
  scale_max: number;
  categories: [string, number][];
}

export interface StarterPick {
  modality: Modality;
  model_id: string;
  model_name: string;
  variant_id: string;
  variant_label: string;
  size: number;
  fit: FitReport | null;
  speed?: SpeedReport | null;
  installed: boolean;
}

export type EnqueueOutcome =
  | { kind: 'queued'; files: number; bytes: number; reused: number }
  | { kind: 'already_installed'; reused: number };

// --- downloads -----------------------------------------------------------------

export type DownloadStatus = 'queued' | 'downloading' | 'verifying' | 'paused' | 'completed' | 'failed';

export interface DownloadItem {
  id: number;
  model_id: string;
  variant_id: string;
  repo: string;
  file_path: string;
  url: string;
  dest: string;
  size: number;
  sha256: string | null;
  gated: boolean;
  downloaded: number;
  status: DownloadStatus;
  error: string | null;
  attempts: number;
  position: number;
}

export interface Progress {
  id: number;
  model_id: string;
  variant_id: string;
  downloaded: number;
  total: number;
  bytes_per_sec: number;
  status: DownloadStatus;
}

// --- engines & chat (M2) -----------------------------------------------------------

export type EngineState = 'not_installed' | 'installing' | 'stopped' | 'starting' | 'running' | 'crashed';

export interface LoadedModel {
  model_id: string;
  variant_id: string;
  name: string;
  memory_estimate_mb: number;
  since: number;
}

export interface EngineStatus {
  id: string;
  name: string;
  version: string;
  capabilities: string[];
  license: string;
  state: EngineState;
  installed: { build_id: string; version: string; backend: string; dir: string; installed_at: number } | null;
  update_available: boolean;
  builds: { id: string; backend: string; size: number; recommended: boolean }[];
  install_progress: [number, number] | null;
  loaded: LoadedModel | null;
  error: string | null;
  log_path: string;
  /** `python`: an environment created by uv (ADR 0018). */
  kind?: 'native' | 'python';
  /** Python engines while installing: uv, python, packages, verify. */
  stage?: string | null;
  /** Python engines: size of the environment on disk. */
  disk_size?: number | null;
}

export interface Session {
  id: number;
  title: string;
  pinned: boolean;
  created_at: number;
  updated_at: number;
  messages: number;
}

export type MessageStatus = 'queued' | 'preparing' | 'loading_model' | 'running' | 'done' | 'failed' | 'cancelled';

export interface Message {
  id: number;
  session_id: number;
  role: 'system' | 'user' | 'assistant';
  content: string;
  reasoning: string;
  model_id: string | null;
  variant_id: string | null;
  status: MessageStatus;
  error: string | null;
  tokens: number | null;
  tokens_per_second: number | null;
  created_at: number;
  finished_at: number | null;
  tool_calls: ToolCallRecord[];
}

export type ToolCallStatus = 'pending' | 'running' | 'done' | 'error' | 'denied';

export interface ToolCallRecord {
  id: string;
  function_name: string;
  server_name: string;
  tool: string;
  remote: boolean;
  arguments: string;
  status: ToolCallStatus;
  result: string | null;
  duration_ms: number | null;
}

// --- MCP (M2b) ---------------------------------------------------------------

export type ToolPolicy = 'ask' | 'allow' | 'block';

export interface KeyValue {
  key: string;
  value: string;
  secret: boolean;
}

export type McpTransport =
  | { kind: 'stdio'; command: string; args: string[]; env: KeyValue[]; cwd: string | null }
  | { kind: 'http'; url: string; bearer: string | null; headers: KeyValue[] };

export interface McpServerConfig {
  id: number;
  name: string;
  transport: McpTransport;
  enabled: boolean;
  policies: Record<string, ToolPolicy>;
}

export type McpConnState = 'disconnected' | 'connecting' | 'connected' | 'error';

export interface McpServerStatus {
  config: McpServerConfig;
  remote: boolean;
  state: McpConnState;
  error: string | null;
  server_name: string | null;
  server_version: string | null;
  tools: { name: string; description: string; policy: ToolPolicy }[];
  log_path: string | null;
}

export interface McpTestResult {
  server_name: string;
  server_version: string;
  tools: { name: string; description: string }[];
}

// --- jobs, gallery and memory (M3) ------------------------------------------------

export type JobKind = 'image' | 'stt' | 'tts' | 'music' | 'decision';
export type JobStatus = MessageStatus;

export interface Segment {
  start_ms: number;
  end_ms: number;
  text: string;
}

export interface Job {
  id: number;
  kind: JobKind;
  status: JobStatus;
  progress: number | null;
  model_id: string | null;
  variant_id: string | null;
  model_name: string;
  engine: string;
  /** Image: prompt, negative_prompt, width, height, seed, count, steps, cfg_scale, aspect. Transcription: language.
   * Speech: language, voice_id, voice_name, settings. */
  params: Record<string, unknown>;
  inputs: Record<string, unknown>;
  /** Image: files, seeds, width, height, offload. Transcription: segments, language, audio, audio_duration_ms.
   * Speech: files (WAV, MP3), duration_ms, sample_rate, device, voice_name. */
  outputs: Record<string, unknown>;
  text: string;
  error: string | null;
  created_at: number;
  started_at: number | null;
  finished_at: number | null;
  duration_ms: number | null;
}

export interface JobFilter {
  kind?: JobKind | null;
  query?: string | null;
  model_id?: string | null;
  done_only?: boolean;
  /** Leave out decisions (Gallery). */
  media_only?: boolean;
  since?: number | null;
  limit?: number | null;
}

export interface ImageForm {
  prompt: string;
  negativePrompt?: string;
  aspect: string;
  count: number;
  seed?: number | null;
  steps?: number | null;
  cfgScale?: number | null;
  size?: number | null;
  modelId?: string | null;
  variantId?: string | null;
}

/** Workshop "Głos" form (M4). */
export interface SpeechForm {
  text: string;
  language?: string | null;
  voiceId?: number | null;
  modelId?: string | null;
  variantId?: string | null;
  speed?: number | null;
  exaggeration?: number | null;
  cfgWeight?: number | null;
  temperature?: number | null;
}

/** A song (ADR 0022): style description and lyrics; empty lyrics = instrumental. */
export interface MusicForm {
  caption: string;
  lyrics: string;
  language?: string | null;
  /** Seconds (10–600); null = the model decides. */
  duration?: number | null;
  seed?: number | null;
  modelId?: string | null;
  variantId?: string | null;
}

/** A typed question for a decision model (ADR 0024). */
export type DecisionQuestion =
  | { id: string; instructions: string; type: 'choice'; options: string[] }
  | { id: string; instructions: string; type: 'yes_no' }
  | { id: string; instructions: string; type: 'score'; levels: string[] };

export interface DecisionProbability {
  label: string;
  p: number;
}

/** One answer: the most likely label (`yes`/`no` for yes/no questions) and every probability. */
export interface DecisionAnswer {
  id: string;
  label: string;
  probabilities: DecisionProbability[];
  confidence: number;
  score?: number;
}

/** Workshop "Decyzje" form: the text to judge and the questions. */
export interface DecisionForm {
  state: string;
  questions: DecisionQuestion[];
  modelId?: string | null;
  variantId?: string | null;
}

/** Decide step gate: continue only on `answer` (top answer, or at least `min_percent`). */
export interface DecisionGate {
  answer: string;
  min_percent?: number | null;
}

export type QualityLevel = 'good' | 'fair' | 'poor';
export type QualityIssue = 'too_short' | 'too_long' | 'noisy' | 'clipping' | 'quiet';

export interface SampleQuality {
  level: QualityLevel;
  issues: QualityIssue[];
  duration_ms: number;
  peak_db: number;
  speech_db: number;
  noise_db: number;
  clipped: number;
}

/** A recording opened for trimming (V6). */
export interface SampleInfo {
  path: string;
  duration_ms: number;
  sample_rate: number;
  peaks: number[];
  quality: SampleQuality;
}

export interface Voice {
  id: number;
  name: string;
  language: string;
  model_id: string | null;
  sample_file: string;
  sample_ms: number;
  quality: Partial<SampleQuality>;
  consent_text: string;
  consent_at: number;
  source: 'recording' | 'upload' | 'import';
  created_at: number;
  updated_at: number;
  sample_path: string;
  sample_exists: boolean;
}

export interface VoiceForm {
  path: string;
  startMs: number;
  endMs: number;
  name: string;
  language: string;
  modelId?: string | null;
  consent: boolean;
  consentText: string;
  source: 'recording' | 'upload';
}

export interface MemoryItem {
  key: string;
  kind: 'resident' | 'job';
  engine_id: string;
  name: string;
  memory_mb: number;
  since: number;
  last_used: number;
  keep: boolean;
  job_id?: number;
}

export interface MemoryStatus {
  budget_mb: number | null;
  used_mb: number;
  keep_llm: boolean;
  items: MemoryItem[];
}

export interface JobProgress {
  job_id: number;
  progress: number;
}

export interface ChatSent {
  session: Session;
  user: Message;
  assistant: Message;
}

export interface ChatDelta {
  message_id: number;
  content: string;
  reasoning: string;
}

// --- pipelines (M5) ---------------------------------------------------------------

export type Port = 'text' | 'audio' | 'image' | 'subtitles';
export type StepMode = 'transcribe' | 'chat' | 'speak' | 'image' | 'decide';

export interface ModelRef {
  model_id: string;
  variant_id: string;
}

export interface PipelineStep {
  mode: StepMode;
  model?: ModelRef | null;
  /** Chat: prompt template with {{input}}. */
  prompt?: string | null;
  /** 0 = pipeline input, n = step n (1-based); null = previous step. */
  from?: number | null;
  language?: string | null;
  voice_id?: number | null;
  aspect?: string | null;
  /** Decide: the question; its answer is the step's output. */
  question?: DecisionQuestion | null;
  /** Decide: continue only on this answer; null = always continue. */
  gate?: DecisionGate | null;
}

export interface Pipeline {
  id?: number | null;
  name: string;
  template?: string | null;
  steps: PipelineStep[];
}

export interface PipelineIssue {
  step: number | null;
  code: string;
}

export interface RunInput {
  text?: string | null;
  file?: string | null;
}

export interface StepOutput {
  port: Port;
  text: string;
  files: string[];
  /** Decide steps with a gate: whether the run went on. */
  gate_passed?: boolean | null;
}

export interface StepRun {
  status: JobStatus;
  job_id?: number | null;
  output?: StepOutput | null;
  error?: string | null;
  duration_ms?: number | null;
  /** Not run because a decision gate stopped the run. */
  skipped?: boolean;
}

export interface PipelineRun {
  id: number;
  pipeline_id: number | null;
  name: string;
  status: JobStatus;
  definition: Pipeline;
  input: RunInput;
  steps: StepRun[];
  error: string | null;
  created_at: number;
  finished_at: number | null;
}

// --- local network / phones (M5) -----------------------------------------------------

export type PairingState = 'waiting' | 'confirm' | 'paired' | 'rejected' | 'expired' | 'cancelled';

export interface PairingView {
  state: PairingState;
  uri: string;
  /** Modules row by row, `1` = dark. */
  qr: { size: number; modules: string } | null;
  expires_in: number;
  code: string | null;
  device_name: string | null;
}

export interface PairedDevice {
  id: string;
  name: string;
  public_key: string;
  paired_at: number;
  last_seen: number | null;
}

export interface NetworkStatus {
  enabled: boolean;
  port: number;
  running: boolean;
  address: string | null;
  error: string | null;
  devices: PairedDevice[];
  pairing: PairingView | null;
}

/** An access key of the local server (ADR 0025); the secret is never listed. */
export interface ApiKey {
  id: string;
  name: string;
  agent: string | null;
  created_at: number;
  last_used_at: number | null;
  requests: number;
}

export interface ServerStatus {
  enabled: boolean;
  port: number;
  lan: boolean;
  running: boolean;
  /** `http://127.0.0.1:<port>/v1` while running. */
  url: string | null;
  lan_url: string | null;
  error: string | null;
  keys: ApiKey[];
}

/** A program that may use Lokalomat through MCP (ADR 0034). */
export interface McpClient {
  key_id: string;
  key_name: string;
  name: string;
  version: string | null;
  allowed_at: number;
  last_seen_at: number | null;
}

/** The question "may this program use Lokalomat?". */
export interface McpConsentRequest {
  id: string;
  client: string;
  version: string | null;
  key_name: string;
}

/** Commands and snippets that connect an MCP program with one key. */
export interface McpSetup {
  url: string;
  key_name: string;
  claude_found: boolean;
  bridge_path: string | null;
  http_command: string;
  stdio_command: string | null;
  http_json: string;
  stdio_json: string | null;
}

/** One line of "Ostatnie zapytania" (no prompt or answer content). */
export interface ServerLogEntry {
  at: number;
  key: string;
  method: string;
  path: string;
  model: string | null;
  status: number;
  duration_ms: number;
  stream: boolean;
}

/** Install state of a third-party agent (ADR 0026). */
export interface AgentStatus {
  id: string;
  installed: boolean;
  version: string | null;
  installing: boolean;
  progress: [number, number] | null;
  error: string | null;
}

/** An agent from `catalog/agents.json` with its status. */
export interface AgentView {
  id: string;
  name: string;
  publisher: string;
  homepage: string;
  license: { name: string; open_source: boolean };
  description_pl: string;
  description_en: string;
  install: { kind: 'binary' | 'npm' };
  needs: { tools: boolean; min_context: number; account: boolean; user_settings?: boolean };
  status: AgentStatus;
}

// --- errors ---------------------------------------------------------------------

/** Error returned by M1+ commands: `code` selects the translated message. */
export interface CommandError {
  code: string;
  message: string;
  needed?: number;
  available?: number;
}

export function isCommandError(e: unknown): e is CommandError {
  return typeof e === 'object' && e !== null && 'code' in e && 'message' in e;
}

export const DEFAULT_SETTINGS: AppSettings = {
  language: 'pl',
  theme: 'system',
  log_mode: 'normal',
  log_prompts: false,
  data_dir: null,
  onboarded: false,
  bandwidth_limit_kbps: null,
  unified_memory_percent: 70,
  expert: false,
  context_size: null,
  keep_llm_loaded: false,
  lan_enabled: false,
  lan_port: 47821,
  server_enabled: false,
  server_port: 47822,
  server_lan: false,
  update_check: true,
  send_key: 'ctrl_enter',
};

export const inTauri: boolean = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export class DesktopOnlyError extends Error {
  constructor(command: string) {
    super(`"${command}" is only available in the desktop app`);
    this.name = 'DesktopOnlyError';
  }
}

const FALLBACK_KEY = 'lokalsi.browser-settings';

function browserSettingsView(): SettingsView {
  const settings = { ...DEFAULT_SETTINGS, ...readLocal<Partial<AppSettings>>(FALLBACK_KEY, {}) };
  return { settings, effective_log_mode: settings.log_mode, log_mode_overridden: false };
}

export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) return invoke<T>(command, args);
  switch (command) {
    case 'get_settings':
      return browserSettingsView() as T;
    case 'update_settings':
      writeLocal(FALLBACK_KEY, args?.settings);
      return browserSettingsView() as T;
    case 'set_data_dir':
      writeLocal(FALLBACK_KEY, { ...browserSettingsView().settings, data_dir: args?.path });
      return browserSettingsView() as T;
    case 'log_from_ui':
      console.debug('[log_from_ui]', args);
      return undefined as T;
    default: {
      const { mockCall, UNSUPPORTED } = await import('./mock');
      const result = await mockCall(command, args);
      if (result === UNSUPPORTED) throw new DesktopOnlyError(command);
      return result as T;
    }
  }
}

export const ipc = {
  getSettings: () => call<SettingsView>('get_settings'),
  updateSettings: (settings: AppSettings) => call<SettingsView>('update_settings', { settings }),
  getAppInfo: () => call<AppInfo>('get_app_info'),
  getEdition: () => call<Edition>('get_edition'),
  checkUpdate: () => call<UpdateInfo | null>('check_update'),
  installUpdate: () => call<void>('install_update'),
  logFromUi: (level: LogLevel, message: string, context?: string) =>
    call<void>('log_from_ui', { level, message, context: context ?? null }),
  getLogTail: (lines: number) => call<string>('get_log_tail', { lines }),
  openLogDir: () => call<void>('open_log_dir'),
  exportDiagnostics: (dest: string) => call<ExportSummary>('export_diagnostics', { dest }),
  getHardware: (refresh = false) => call<HardwareView>('get_hardware', { refresh }),
  getStorageOptions: () => call<StorageOptions>('get_storage_options'),
  setDataDir: (path: string) => call<SettingsView>('set_data_dir', { path }),
  openDataDir: () => call<void>('open_data_dir'),
  listModels: () => call<ModelView[]>('list_models'),
  getStarterPack: () => call<StarterPick[]>('get_starter_pack'),
  downloadVariant: (modelId: string, variantId: string) =>
    call<EnqueueOutcome>('download_variant', { modelId, variantId }),
  deleteVariant: (modelId: string, variantId: string) => call<void>('delete_variant', { modelId, variantId }),
  setDefaultModel: (modelId: string, variantId: string) =>
    call<void>('set_default_model', { modelId, variantId }),
  addCustomModelFile: (path: string) => call<ModelView>('add_custom_model_file', { path }),
  addCustomModelHf: (repo: string, path: string) => call<EnqueueOutcome>('add_custom_model_hf', { repo, path }),
  removeCustomModel: (id: string) => call<void>('remove_custom_model', { id }),
  listDownloads: () => call<DownloadItem[]>('list_downloads'),
  pauseDownload: (id: number) => call<void>('pause_download', { id }),
  resumeDownload: (id: number) => call<void>('resume_download', { id }),
  cancelDownload: (id: number) => call<void>('cancel_download', { id }),
  cancelVariantDownload: (modelId: string, variantId: string) =>
    call<void>('cancel_variant_download', { modelId, variantId }),
  moveDownload: (id: number, up: boolean) => call<void>('move_download', { id, up }),
  clearFinishedDownloads: () => call<number>('clear_finished_downloads'),
  getHfTokenStatus: () => call<boolean>('get_hf_token_status'),
  getBenchmarkMeta: () => call<BenchmarkMeta | null>('get_benchmark_meta'),
  getEngines: () => call<EngineStatus[]>('get_engines'),
  installEngine: (engineId: string, buildId?: string) =>
    call<void>('install_engine', { engineId, buildId: buildId ?? null }),
  cancelEngineInstall: (engineId: string) => call<void>('cancel_engine_install', { engineId }),
  uninstallEngine: (engineId: string) => call<void>('uninstall_engine', { engineId }),
  stopEngine: () => call<void>('stop_engine'),
  restartEngine: (engineId: string) => call<void>('restart_engine', { engineId }),
  getEngineLog: (engineId: string, lines: number) => call<string>('get_engine_log', { engineId, lines }),
  ensureEngines: () => call<void>('ensure_engines'),
  listSessions: (query?: string) => call<Session[]>('list_sessions', { query: query ?? null }),
  getMessages: (sessionId: number) => call<Message[]>('get_messages', { sessionId }),
  renameSession: (id: number, title: string) => call<void>('rename_session', { id, title }),
  pinSession: (id: number, pinned: boolean) => call<void>('pin_session', { id, pinned }),
  deleteSession: (id: number) => call<void>('delete_session', { id }),
  chatSend: (sessionId: number | null, text: string, model?: { modelId: string; variantId: string }) =>
    call<ChatSent>('chat_send', { sessionId, text, modelId: model?.modelId ?? null, variantId: model?.variantId ?? null }),
  chatCancel: (messageId: number) => call<boolean>('chat_cancel', { messageId }),
  getModelParams: (modelId: string, variantId: string) => call<ModelParams>('get_model_params', { modelId, variantId }),
  setModelParams: (modelId: string, variantId: string, params: ModelParams) =>
    call<ModelParams>('set_model_params', { modelId, variantId, params }),
  getModelLayers: (modelId: string, variantId: string) => call<number | null>('get_model_layers', { modelId, variantId }),
  setHfToken: (token: string) => call<boolean>('set_hf_token', { token }),
  getMemory: () => call<MemoryStatus>('get_memory'),
  releaseMemory: (key: string) => call<boolean>('release_memory', { key }),
  listJobs: (filter: JobFilter = {}) => call<Job[]>('list_jobs', { filter }),
  cancelJob: (id: number) => call<boolean>('cancel_job', { id }),
  deleteJobs: (ids: number[]) => call<number>('delete_jobs', { ids }),
  repairEngine: (engineId: string) => call<void>('repair_engine', { engineId }),
  decide: (form: DecisionForm) => call<Job>('decide', { form }),
  getServer: () => call<ServerStatus>('get_server'),
  setServer: (enabled: boolean, lan?: boolean, port?: number) =>
    call<ServerStatus>('set_server', { enabled, lan: lan ?? null, port: port ?? null }),
  createApiKey: (name: string) => call<{ key: ApiKey; secret: string }>('create_api_key', { name }),
  apiKeySecret: (id: string) => call<string>('api_key_secret', { id }),
  revokeApiKey: (id: string) => call<ServerStatus>('revoke_api_key', { id }),
  serverLog: () => call<ServerLogEntry[]>('server_log'),
  getNetwork: () => call<NetworkStatus>('get_network'),
  setLanSharing: (enabled: boolean, port?: number) => call<NetworkStatus>('set_lan_sharing', { enabled, port: port ?? null }),
  startPairing: () => call<PairingView>('start_pairing'),
  confirmPairing: (codesMatch: boolean) => call<boolean>('confirm_pairing', { codesMatch }),
  cancelPairing: () => call<void>('cancel_pairing'),
  unpairDevice: (id: string) => call<NetworkStatus>('unpair_device', { id }),
};

export const EVENTS = {
  downloadsChanged: 'downloads://changed',
  downloadsProgress: 'downloads://progress',
  modelInstalled: 'models://installed',
  hardwareUpdated: 'hardware://updated',
  benchmarkUpdated: 'benchmark://updated',
  enginesChanged: 'engines://changed',
  chatDelta: 'chat://delta',
  chatUpdated: 'chat://updated',
  jobsUpdated: 'jobs://updated',
  jobsProgress: 'jobs://progress',
  memoryChanged: 'memory://changed',
  pairingChanged: 'network://pairing',
  devicesChanged: 'network://devices',
  updateAvailable: 'update://available',
  updateProgress: 'update://progress',
} as const;

/** Subscribes to a backend event; a no-op outside Tauri. */
export async function onEvent<T>(name: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  if (!inTauri) return () => undefined;
  return listen<T>(name, (event) => handler(event.payload));
}

export function errorMessage(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (isCommandError(e)) return e.message;
  return typeof e === 'string' ? e : JSON.stringify(e);
}
