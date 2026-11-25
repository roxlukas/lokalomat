/**
 * IPC of the Android client (apps/mobile): every call goes through the
 * phone's Rust side to the paired computer over pinned TLS.
 */
import { invoke } from '@tauri-apps/api/core';
import { t } from '../lib/i18n/index.svelte';
import type { DecisionAnswer, DecisionQuestion } from '../lib/ipc';

export type { DecisionAnswer, DecisionQuestion };

export interface MobileStatus {
  paired: boolean;
  /** The computer in use (its certificate fingerprint). */
  id: string | null;
  computer_name: string | null;
  address: string | null;
}

/** A paired computer. */
export interface Computer {
  id: string;
  name: string;
  address: string;
}

export interface Computers {
  /** The computer in use. */
  active: string | null;
  list: Computer[];
}

export interface Turn {
  role: 'user' | 'assistant' | 'system';
  content: string;
}

export interface Transcription {
  text: string;
  language: string | null;
  segments: { start_ms: number; end_ms: number; text: string }[];
  job_id: number | null;
}

export interface RemoteVoice {
  id: number;
  name: string;
  language: string;
}

export interface GalleryItem {
  job_id: number;
  kind: 'image' | 'stt' | 'tts' | 'music';
  text: string;
  model: string;
  created_at: number;
  files: number;
  /** The job's settings on the computer, for "Użyj ponownie" (ADR 0037). */
  params?: Record<string, unknown>;
}

/** `POST /v1/jobs/image` – the gateway's shape (snake_case), ADR 0037. */
export interface ImageJob {
  prompt: string;
  aspect: string;
  model: string | null;
  negative_prompt?: string;
  count?: number;
  seed?: number | null;
  steps?: number | null;
  cfg_scale?: number | null;
}

export interface MusicJob {
  caption: string;
  lyrics: string;
  language: string | null;
  duration: number | null;
  model: string | null;
  seed?: number | null;
}

export interface SpeechJob {
  input: string;
  voice_id: number | null;
  language: string | null;
  model: string | null;
  speed?: number | null;
}

/** A model installed on the computer that the phone may pick for a mode. */
export interface ModelOption {
  /** `<model id>:<variant id>`. */
  id: string;
  name: string;
  default: boolean;
}

export type Mode = 'chat' | 'stt' | 'tts' | 'image' | 'music' | 'decision';

export type JobStatus = 'queued' | 'preparing' | 'loading_model' | 'running' | 'done' | 'failed' | 'cancelled';

/** A job the phone started on the computer (ADR 0027); poll it with `api.job`. */
export interface JobView {
  job_id: number;
  kind: 'image' | 'music' | 'tts' | 'decision';
  status: JobStatus;
  /** 0–1 while running, when known. */
  progress: number | null;
  /** Error code of a failed job (`jobs.errors.*`). */
  error: string | null;
  model: string;
  /** Result files: download with `galleryFile(job_id, index)`. */
  files: { index: number; content_type: string }[];
  answers: DecisionAnswer[];
}

export function finished(job: JobView): boolean {
  return job.status === 'done' || job.status === 'failed' || job.status === 'cancelled';
}

export interface FileData {
  content_type: string;
  data: string;
}

export interface MobileError {
  code: string;
  message: string;
}

export const inApp: boolean = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!inApp) return Promise.reject({ code: 'not_in_app', message: command } satisfies MobileError);
  return invoke<T>(command, args);
}

export const api = {
  status: () => call<MobileStatus>('status'),
  deviceName: () => call<string | null>('device_name'),
  pairBegin: (link: string, deviceName: string) =>
    call<{ code: string; computer_name: string }>('pair_begin', { link, deviceName }),
  pairWait: () => call<MobileStatus>('pair_wait'),
  pairCancel: () => call<void>('pair_cancel'),
  computers: () => call<Computers>('computers'),
  computerSelect: (id: string) => call<MobileStatus>('computer_select', { id }),
  computerRemove: (id: string) => call<MobileStatus>('computer_remove', { id }),
  info: () => call<{ computer_name: string; version: string; modes: string[]; expert?: boolean; pro?: boolean }>('info'),
  models: (mode: Mode) => call<ModelOption[]>('models', { mode }),
  chat: (messages: Turn[], model: string | null) => call<string>('chat', { messages, model }),
  transcribe: (audio: string, ext: string, language: string | null, model: string | null) =>
    call<Transcription>('transcribe', { audio, ext, language, model }),
  speak: (text: string, voiceId: number | null, language: string | null, model: string | null) =>
    call<string>('speak', { text, voiceId, language, model }),
  image: (prompt: string, aspect: string, model: string | null) =>
    call<string[]>('image', { prompt, aspect, model }),
  startImage: (req: ImageJob) => call<JobView>('start_image', { req }),
  startMusic: (req: MusicJob) => call<JobView>('start_music', { req }),
  startSpeech: (req: SpeechJob) => call<JobView>('start_speech', { req }),
  startDecision: (text: string, questions: DecisionQuestion[], model: string | null) =>
    call<JobView>('start_decision', { text, questions, model }),
  job: (jobId: number) => call<JobView>('job', { jobId }),
  cancelJob: (jobId: number) => call<JobView>('cancel_job', { jobId }),
  /** Opens the system save dialog; false when the user closed it. */
  saveFile: (jobId: number, index: number, fileName: string) =>
    call<boolean>('save_file', { jobId, index, fileName }),
  voices: () => call<RemoteVoice[]>('voices'),
  gallery: (limit: number) => call<GalleryItem[]>('gallery', { limit }),
  galleryThumb: (jobId: number, index: number) => call<FileData>('gallery_thumb', { jobId, index }),
  galleryFile: (jobId: number, index: number) => call<FileData>('gallery_file', { jobId, index }),
  deleteGalleryItem: (jobId: number) => call<void>('delete_gallery_item', { jobId }),
};

const KNOWN = [
  'unreachable',
  'local_network',
  'tls',
  'timeout',
  'connection',
  'unauthorized',
  'pairing_rejected',
  'pairing_expired',
  'pairing_busy',
  'bad_token',
  'bad_link',
  'not_paired',
  'no_model',
  'no_media_model',
  'no_cloning_model',
  'audio_unsupported',
  'not_in_app',
  'expert_only',
  'empty',
  'text_too_long',
  'decision_invalid',
  'save_failed',
  'download_network',
  'download_status',
  'download_io',
  'download_space',
  'download_checksum',
  'download_running',
  'model_unknown',
  'model_not_installed',
  'model_not_synced',
  'local_failed',
  'local_load_failed',
];

const OFFLINE = ['unreachable', 'local_network', 'timeout', 'connection', 'tls'];

/** True when the error means the phone cannot reach the computer right now. */
export function isOffline(e: unknown): boolean {
  const code = typeof e === 'object' && e && 'code' in e ? String((e as MobileError).code) : '';
  return OFFLINE.includes(code);
}

/** Short message for an error from the computer or the connection. */
export function describe(e: unknown): string {
  const code = typeof e === 'object' && e && 'code' in e ? String((e as MobileError).code) : '';
  return KNOWN.includes(code) ? t(`mobile.errors.${code}`) : t('mobile.errors.generic');
}

/** Short message for a failed job (`jobs.errors.*` codes from the computer). */
export function describeJob(job: JobView): string {
  if (job.status === 'cancelled') return t('jobs.errors.interrupted');
  const code = job.error ?? '';
  return JOB_ERRORS.includes(code) ? t(`jobs.errors.${code}`) : t('mobile.jobs.failed');
}

const JOB_ERRORS = [
  'engine_install_failed',
  'engine_start_failed',
  'engine_crashed',
  'model_files_missing',
  'engine_unavailable',
  'out_of_memory',
  'no_result',
  'context_full',
  'not_decision_model',
];

/** A file from the computer as an object URL (revoke it when done). */
export function fileUrl(f: FileData): string {
  const bytes = Uint8Array.from(atob(f.data), (c) => c.charCodeAt(0));
  return URL.createObjectURL(new Blob([bytes], { type: f.content_type }));
}

/** Bytes → base64 (recordings sent to the computer). */
export function toBase64(bytes: Uint8Array): string {
  let s = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) s += String.fromCharCode(...bytes.subarray(i, i + chunk));
  return btoa(s);
}
