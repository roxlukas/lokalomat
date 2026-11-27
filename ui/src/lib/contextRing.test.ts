import { describe, expect, it } from 'vitest';
import type { ContextUsage, Message } from './ipc';
import { arcs, estimateTokens, formatTokens, latestUsage, levelOf, RING_C, ringState } from './contextRing';

const usage = (prompt: number, completion: number, size: number): ContextUsage => ({
  prompt_tokens: prompt,
  completion_tokens: completion,
  ctx_size: size,
  messages: prompt,
  system: 0,
  tools: 0,
});

const msg = (id: number, role: Message['role'], context: ContextUsage | null = null): Message => ({
  id,
  session_id: 1,
  role,
  content: '',
  reasoning: '',
  model_id: null,
  variant_id: null,
  status: 'done',
  error: null,
  tokens: null,
  tokens_per_second: null,
  created_at: 0,
  finished_at: null,
  tool_calls: [],
  context,
});

describe('context ring', () => {
  it('estimates typed text', () => {
    expect(estimateTokens('')).toBe(0);
    expect(estimateTokens('   ')).toBe(0);
    expect(estimateTokens('abcdefg')).toBe(2);
  });

  it('takes the newest answer with numbers', () => {
    const a = usage(100, 10, 4096);
    const b = usage(300, 20, 4096);
    expect(latestUsage([msg(1, 'user'), msg(2, 'assistant', a), msg(3, 'user'), msg(4, 'assistant', b)])).toBe(b);
    // A cancelled answer without numbers keeps the previous value.
    expect(latestUsage([msg(2, 'assistant', a), msg(4, 'assistant')])).toBe(a);
    expect(latestUsage([])).toBeNull();
  });

  it('adds the answer and clips the pending part', () => {
    const s = ringState(usage(700, 100, 1000), null, 500)!;
    expect(s.used).toBe(800);
    expect(s.usedShare).toBeCloseTo(0.8);
    expect(s.pendingShare).toBeCloseTo(0.2);
    expect(s.level).toBe('warn');
    expect(s.totalLevel).toBe('crit');
  });

  it('uses the loaded model before the first answer, hides without a size', () => {
    const s = ringState(null, 8192, 0)!;
    expect(s.size).toBe(8192);
    expect(s.used).toBe(0);
    expect(ringState(null, null, 50)).toBeNull();
  });

  it('starts the pending arc where the used arc ends', () => {
    const a = arcs({ usedShare: 0.25, pendingShare: 0.5 });
    expect(a.used.offset).toBeCloseTo(RING_C * 0.75);
    expect(a.pending.offset).toBeCloseTo(-RING_C * 0.25);
    expect(a.pending.array).toBe(`${RING_C * 0.5} ${RING_C * 0.5}`);
  });

  it('levels and token format', () => {
    expect([levelOf(0.5), levelOf(0.8), levelOf(0.95)]).toEqual(['ok', 'warn', 'crit']);
    expect(formatTokens(850, 'en')).toBe('850');
    expect(formatTokens(12345, 'en')).toBe('12.3k');
  });
});
