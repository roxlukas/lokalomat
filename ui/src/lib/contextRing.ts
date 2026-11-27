// The ring under the chat input (ADR 0041): how much of the model's context
// window the conversation uses and how much the typed text will add.
import type { ContextUsage, Message } from './ipc';

/** Circumference of the ring (r = 6.5). */
export const RING_C = 2 * Math.PI * 6.5;

/** Rough tokens of typed text: BPE tokenizers give about 3.5 characters per token in Polish. */
export function estimateTokens(text: string): number {
  const chars = text.trim().length;
  return chars ? Math.ceil(chars / 3.5) : 0;
}

/** The newest answer that knows its context use. */
export function latestUsage(messages: Message[]): ContextUsage | null {
  for (let i = messages.length - 1; i >= 0; i--) {
    const c = messages[i]!.context;
    if (messages[i]!.role === 'assistant' && c) return c;
  }
  return null;
}

export type RingLevel = 'ok' | 'warn' | 'crit';

export interface RingState {
  used: number;
  size: number;
  pending: number;
  /** Fractions of the window, `pendingShare` clipped to what is left. */
  usedShare: number;
  pendingShare: number;
  /** Colour of the arc: from the used part only. */
  level: RingLevel;
  /** Colour of the value in the tooltip: used and pending together. */
  totalLevel: RingLevel;
  usage: ContextUsage | null;
}

export function levelOf(share: number): RingLevel {
  return share >= 0.9 ? 'crit' : share >= 0.8 ? 'warn' : 'ok';
}

/**
 * State of the ring. `fallbackSize` is the window of the loaded model, used
 * before the first answer; `null` when nothing tells the window size.
 */
export function ringState(usage: ContextUsage | null, fallbackSize: number | null, pending: number): RingState | null {
  const size = usage?.ctx_size || fallbackSize || 0;
  if (size <= 0) return null;
  const used = usage ? usage.prompt_tokens + usage.completion_tokens : 0;
  const usedShare = Math.min(1, used / size);
  const pendingShare = Math.min(1 - usedShare, pending / size);
  return {
    used,
    size,
    pending,
    usedShare,
    pendingShare,
    level: levelOf(usedShare),
    totalLevel: levelOf(usedShare + pendingShare),
    usage,
  };
}

/** `stroke-dasharray` / `stroke-dashoffset` of both arcs. */
export function arcs(s: Pick<RingState, 'usedShare' | 'pendingShare'>): {
  used: { array: string; offset: number };
  pending: { array: string; offset: number };
} {
  const seg = RING_C * s.pendingShare;
  return {
    used: { array: `${RING_C} ${RING_C}`, offset: RING_C * (1 - s.usedShare) },
    // Starts where the used arc ends.
    pending: { array: `${seg} ${RING_C - seg}`, offset: -RING_C * s.usedShare },
  };
}

/** `12.3k`, `850` */
export function formatTokens(n: number, locale = 'pl'): string {
  if (n < 1000) return Math.round(n).toLocaleString(locale);
  return `${(n / 1000).toLocaleString(locale, { maximumFractionDigits: 1, minimumFractionDigits: 1 })}k`;
}
