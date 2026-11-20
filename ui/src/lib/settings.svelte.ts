import { setLocale } from './i18n/index.svelte';
import { DEFAULT_SETTINGS, ipc, type AppSettings, type LogMode, type SettingsView, type Theme } from './ipc';

export function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  if (theme === 'system') root.removeAttribute('data-theme');
  else root.setAttribute('data-theme', theme);
}

export function resolvedTheme(theme: Theme): 'light' | 'dark' {
  if (theme !== 'system') return theme;
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

class SettingsStore {
  value = $state<AppSettings>({ ...DEFAULT_SETTINGS });
  effectiveLogMode = $state<LogMode>('normal');
  logModeOverridden = $state(false);
  loaded = $state(false);

  async load(): Promise<void> {
    this.apply(await ipc.getSettings());
    this.loaded = true;
  }

  /** Persists a change; on failure the previous values stay and the error is rethrown. */
  async update(patch: Partial<AppSettings>): Promise<void> {
    const next = { ...this.value, ...patch };
    if (next.log_mode !== 'verbose') next.log_prompts = false;
    this.apply(await ipc.updateSettings(next));
  }

  /** Applies a view returned by any command that changed settings (e.g. `set_data_dir`). */
  apply(view: SettingsView): void {
    this.value = view.settings;
    this.effectiveLogMode = view.effective_log_mode;
    this.logModeOverridden = view.log_mode_overridden;
    setLocale(view.settings.language);
    applyTheme(view.settings.theme);
  }
}

export const settings = new SettingsStore();
