import { getLocale, t } from './i18n/index.svelte';
import { errorMessage, isCommandError, type EnqueueOutcome } from './ipc';
import { formatBytes } from './format';

const KNOWN_ERRORS = [
  'insufficient_space',
  'no_data_dir',
  'not_gguf',
  'hf_not_found',
  'hf_auth',
  'hf_invalid',
  'network',
  'not_writable',
  'invalid_path',
  'secret_store',
  'engine_missing',
  'no_model',
  'engine_start_failed',
  'engine_missing_library',
  'empty',
  'mcp_invalid',
  'mcp_connect',
  'no_media_model',
  'consent_required',
  'no_name',
  'sample_too_short',
  'invalid_voice',
  'audio_unsupported',
  'no_cloning_model',
  'voice_unsupported',
  'text_too_long',
  'pipeline_invalid',
  'pipeline_missing_input',
  'lan_disabled',
  'decision_invalid',
  'agent_install_failed',
  'agent_not_installed',
  'agent_setup_failed',
  'agent_start_failed',
  'no_terminal',
  'agent_context_small',
  'update_offline',
  'update_failed',
  'update_manual',
  'update_unavailable',
  'server_off',
  'key_not_stored',
  'claude_missing',
  'bridge_missing',
  'claude_failed',
];

/** User-facing text for a command error (translated by `code`). */
export function describeError(e: unknown): string {
  if (isCommandError(e) && KNOWN_ERRORS.includes(e.code)) {
    const loc = getLocale();
    return t(`errors.${e.code}`, {
      needed: formatBytes(e.needed ?? 0, loc),
      available: formatBytes(e.available ?? 0, loc),
      package: e.message,
    });
  }
  return t('errors.generic', { error: errorMessage(e) });
}

/** Parameters of a stored `code: detail` error (the detail of
 * `engine_missing_library` is the package to install, ADR 0017). */
export function storedErrorParams(error: string): Record<string, string> {
  const i = error.indexOf(':');
  return { package: i < 0 ? '' : error.slice(i + 1).trim() };
}

/** Translated download error stored as `key` or `key: details`. */
export function describeDownloadError(error: string | null): string {
  if (!error) return '';
  const key = error.split(':')[0]!.trim();
  const text = t(`downloads.errors.${key}`);
  return text === `downloads.errors.${key}` ? error : text;
}

export function describeEnqueue(outcome: EnqueueOutcome): string {
  return outcome.kind === 'queued'
    ? t('models.enqueued', { size: formatBytes(outcome.bytes, getLocale()) })
    : t('models.alreadyInstalled');
}
