/**
 * Hides the reasoning a model writes before its answer (`<think>…</think>`,
 * e.g. DeepSeek R1). Pieces arrive in a stream, so a tag may be split between
 * two pieces; `push` returns only the text to show.
 */
const OPEN = '<think>';
const CLOSE = '</think>';

/** Length of the longest end of `text` that is a start of `tag`. */
function partialTag(text: string, tag: string): number {
  for (let n = Math.min(tag.length - 1, text.length); n > 0; n--) {
    if (tag.startsWith(text.slice(text.length - n))) return n;
  }
  return 0;
}

export class ThinkFilter {
  private inside: boolean;
  /** Text of the answer has started (leading spaces are dropped until then). */
  private started = false;
  /** Held back: it may be the start of a tag. */
  private pending = '';
  private hidden = '';

  /** `thinks`: the model starts inside `<think>` (the tag may be in the prompt). */
  constructor(thinks: boolean) {
    this.inside = thinks;
  }

  /** True while the model is still reasoning. */
  get thinking(): boolean {
    return this.inside;
  }

  push(piece: string): string {
    let text = this.pending + piece;
    this.pending = '';
    let out = '';
    while (text) {
      if (this.inside) {
        const close = text.indexOf(CLOSE);
        if (close >= 0) {
          this.hidden += text.slice(0, close);
          text = text.slice(close + CLOSE.length);
          this.inside = false;
          this.started = false;
          continue;
        }
        const keep = partialTag(text, CLOSE);
        this.hidden += text.slice(0, text.length - keep);
        this.pending = text.slice(text.length - keep);
        break;
      }
      if (!this.started) {
        const trimmed = text.trimStart();
        if (!trimmed) break;
        if (trimmed.startsWith(OPEN)) {
          this.inside = true;
          text = trimmed.slice(OPEN.length);
          continue;
        }
        if (OPEN.startsWith(trimmed)) {
          this.pending = trimmed;
          break;
        }
        this.started = true;
        text = trimmed;
      }
      out += text;
      break;
    }
    return out;
  }

  /**
   * The rest to show when the reply ends. A model that never closed its
   * reasoning shows it after all, so the reply is not lost.
   */
  finish(): string {
    const rest = this.inside ? this.hidden + this.pending : this.pending;
    this.hidden = '';
    this.pending = '';
    return rest.replace(OPEN, '').trim();
  }
}
