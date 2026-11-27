import { describe, expect, it } from 'vitest';
import { CONTEXT_MAX, CONTEXT_SIZES, contextChoice, contextChoices, contextOption } from './contextSizes';

describe('context choices', () => {
  it('maps automatic, fixed and max between settings and the select', () => {
    expect(contextOption(null)).toBe('auto');
    expect(contextChoice('auto')).toBeNull();
    expect(contextChoice(contextOption(32768))).toBe(32768);
    expect(contextChoice(contextOption(CONTEXT_MAX))).toBe(CONTEXT_MAX);
  });

  it('offers 8k to 128k and keeps an older saved value visible', () => {
    expect(CONTEXT_SIZES).toEqual([8192, 16384, 32768, 65536, 131072]);
    expect(contextChoices(null)).toEqual([...CONTEXT_SIZES]);
    expect(contextChoices(CONTEXT_MAX)).toEqual([...CONTEXT_SIZES]);
    expect(contextChoices(4096)[0]).toBe(4096);
  });
});
