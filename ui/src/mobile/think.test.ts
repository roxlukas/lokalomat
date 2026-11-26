import { describe, expect, it } from 'vitest';
import { ThinkFilter } from './think';

function run(thinks: boolean, pieces: string[]): { shown: string; thinkingAfter: boolean[] } {
  const f = new ThinkFilter(thinks);
  let shown = '';
  const thinkingAfter: boolean[] = [];
  for (const p of pieces) {
    shown += f.push(p);
    thinkingAfter.push(f.thinking);
  }
  return { shown: shown + f.finish(), thinkingAfter };
}

describe('ThinkFilter', () => {
  it('passes a plain answer through', () => {
    expect(run(false, ['Hello', ' world']).shown).toBe('Hello world');
  });

  it('hides reasoning in tags, even when a tag is split', () => {
    const r = run(false, ['\n<thi', 'nk>Let me see', '. 17*23</th', 'ink>\n\n', '**391**']);
    expect(r.shown).toBe('**391**');
    expect(r.thinkingAfter).toEqual([false, true, true, false, false]);
  });

  it('hides reasoning when the opening tag is in the prompt', () => {
    expect(run(true, ['Okay, the user', ' asks…</think>', 'Answer: 391']).shown).toBe('Answer: 391');
  });

  it('accepts an opening tag from a model that starts inside', () => {
    expect(run(true, ['<think>\nhmm\n</think>\n\nDone']).shown).toBe('Done');
  });

  it('shows the reasoning when it never ends, so the reply is not lost', () => {
    expect(run(true, ['<think>only ', 'thoughts']).shown).toBe('only thoughts');
  });

  it('keeps a "<" that is not a tag', () => {
    expect(run(false, ['<', '3 is a heart']).shown).toBe('<3 is a heart');
    expect(run(false, ['a < b']).shown).toBe('a < b');
  });
});
