/**
 * State the phone's screens share (ADR 0037): whether the computer is in
 * Expert mode, and settings from the Gallery's "Użyj ponownie" waiting for
 * the screen of their mode.
 */
export type ReuseKind = 'image' | 'tts' | 'music';

export interface Reuse {
  kind: ReuseKind;
  /** The job's settings as the computer stored them. */
  params: Record<string, unknown>;
}

const PRO_KEY = 'lokalomat.phone.pro';

function readPro(): boolean {
  try {
    return localStorage.getItem(PRO_KEY) === '1';
  } catch {
    return false;
  }
}

class Phone {
  /** The computer is in Expert mode (`/v1/info`): show the same extra settings. */
  expert = $state(false);
  /** The computer runs Pro: the gold mark. Remembered, so the mark does not flash purple while the app waits for the computer. */
  #pro = $state(readPro());
  get pro(): boolean {
    return this.#pro;
  }
  set pro(value: boolean) {
    this.#pro = value;
    try {
      localStorage.setItem(PRO_KEY, value ? '1' : '0');
    } catch {
      /* storage can be blocked: the mark then follows the computer each time */
    }
  }
  /** Settings to put into a screen; the screen clears it once taken. */
  reuse = $state<Reuse | null>(null);

  /** Takes the waiting settings when they are for `kind`. */
  take(kind: ReuseKind): Record<string, unknown> | null {
    if (this.reuse?.kind !== kind) return null;
    const { params } = this.reuse;
    this.reuse = null;
    return params;
  }
}

export const phone = new Phone();

/** A number from stored settings, or null. */
export function num(params: Record<string, unknown>, key: string): number | null {
  const v = params[key];
  return typeof v === 'number' && Number.isFinite(v) ? v : null;
}

/** A string from stored settings, or ''. */
export function str(params: Record<string, unknown>, key: string): string {
  const v = params[key];
  return typeof v === 'string' ? v : '';
}

/** A whole number typed in a field (empty or invalid = null), kept within `min`–`max`. */
export function wholeOrNull(value: number | null | undefined, min: number, max: number): number | null {
  if (value === null || value === undefined || !Number.isFinite(value)) return null;
  return Math.min(max, Math.max(min, Math.round(value)));
}
