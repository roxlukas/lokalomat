import type { SendKey } from './ipc';

/** The subset of a keyboard event that decides whether a prompt is sent. */
export type SendKeyEvent = Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey' | 'isComposing'>;

/**
 * True when this key press sends the prompt (ADR 0036). Ctrl/⌘+Enter sends in both
 * schemes; with `enter` a plain Enter sends too and Shift+Enter adds a new line.
 * Never while an input method is composing text.
 */
export function isSendKey(e: SendKeyEvent, scheme: SendKey): boolean {
  if (e.key !== 'Enter' || e.isComposing) return false;
  if (e.ctrlKey || e.metaKey) return true;
  return scheme === 'enter' && !e.shiftKey && !e.altKey;
}

/** Shortcut hint for the send button (`title`). */
export function sendKeyHint(scheme: SendKey): string {
  return scheme === 'enter' ? 'Enter' : 'Ctrl+Enter';
}

/** Value for `aria-keyshortcuts` on the prompt field. */
export function sendKeyAria(scheme: SendKey): string {
  return scheme === 'enter' ? 'Enter Control+Enter' : 'Control+Enter';
}
