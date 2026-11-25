/**
 * The model on the phone itself (docs/plans/android-local-chat.md, ADR 0038):
 * the Kotlin plugin `localllm` runs it, the phone's Rust side manages the files.
 */
import { Channel, invoke } from '@tauri-apps/api/core';
import { inApp, type MobileError, type Turn } from './api';

export interface PhoneModel {
  id: string;
  name: string;
  publisher: string;
  description_pl: string;
  description_en: string;
  params_b: number;
  languages: string[];
  license: { name: string; url: string };
  /** Bytes. */
  size: number;
  min_ram_mb: number;
  recommended: boolean;
  /** `low` = less memory than the model needs; null when the phone's RAM is unknown. */
  fit: 'ok' | 'low' | null;
  installed: boolean;
  /** Bytes of an interrupted download. */
  partial_bytes: number;
  downloading: boolean;
}

export interface PhoneModels {
  ram_mb: number | null;
  models: PhoneModel[];
}

export interface Progress {
  downloaded: number;
  total: number;
}

type GenerateEvent =
  | { token: string }
  | { thought: true }
  | { done: true }
  | { cancelled: true }
  | { error: string };

function fail(code: string, message: unknown): MobileError {
  return { code, message: String(message) };
}

function plugin<T = void>(command: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(`plugin:localllm|${command}`, args);
}

export const local = {
  /** True where the plugin exists (the Android app); false in a browser or on a computer. */
  async available(): Promise<boolean> {
    if (!inApp) return false;
    try {
      await plugin('status');
      return true;
    } catch {
      return false;
    }
  },

  models: () => invoke<PhoneModels>('phone_models'),

  download(id: string, onProgress: (p: Progress) => void): Promise<void> {
    const channel = new Channel<Progress>();
    channel.onmessage = onProgress;
    return invoke('phone_model_download', { id, onProgress: channel });
  },
  cancelDownload: (id: string) => invoke<void>('phone_model_cancel', { id }),
  remove: (id: string) => invoke<void>('phone_model_delete', { id }),

  /** Loads an installed model into memory (replaces the loaded one). */
  async load(id: string): Promise<void> {
    const path = await invoke<string>('phone_model_path', { id });
    try {
      // CPU: faster and lighter than the GPU on the phone we measured (ADR 0038).
      await plugin('load', { path, backend: 'cpu' });
    } catch (e) {
      throw fail('local_load_failed', e);
    }
  },

  /**
   * Answers `prompt` after the earlier turns in `history`; every piece of the
   * answer goes to `onToken`. Resolves with `cancelled` when `cancel()` stopped it.
   */
  generate(
    prompt: string,
    history: Turn[],
    onToken: (text: string) => void,
    onThinking: () => void = () => undefined,
  ): Promise<{ cancelled: boolean }> {
    return new Promise((resolve, reject) => {
      const channel = new Channel<GenerateEvent>();
      channel.onmessage = (m) => {
        if ('token' in m) onToken(m.token);
        else if ('thought' in m) onThinking();
        else if ('done' in m) resolve({ cancelled: false });
        else if ('cancelled' in m) resolve({ cancelled: true });
        else reject(fail('local_failed', m.error));
      };
      const past = history
        .filter((h) => h.role === 'user' || h.role === 'assistant')
        .map((h) => ({ role: h.role, content: h.content }));
      plugin('generate', { prompt, history: past, onEvent: channel }).catch((e) =>
        reject(fail('local_failed', e)),
      );
    });
  },
  cancel: () => plugin('cancel'),
  /** Frees the memory of the loaded model. */
  unload: () => plugin('unload'),
};

/** `2 588 147 712` bytes → "2.4 GB". */
export function gigabytes(bytes: number): string {
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}

export function percent(p: Progress | undefined): number {
  return p && p.total > 0 ? Math.min(100, Math.floor((p.downloaded / p.total) * 100)) : 0;
}
