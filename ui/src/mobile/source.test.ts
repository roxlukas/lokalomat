import { describe, expect, it } from 'vitest';
import { gigabytes, percent } from './local';
import { source } from './source.svelte';

describe('where the chat runs', () => {
  it('uses the computer when paired and nothing was chosen', () => {
    source.localAvailable = true;
    expect(source.current(true)).toBe('computer');
  });

  it('uses the phone when there is no computer', () => {
    source.localAvailable = true;
    expect(source.current(false)).toBe('local');
  });

  it('keeps the choice while it is possible', () => {
    source.localAvailable = true;
    source.set('local');
    expect(source.current(true)).toBe('local');
    source.set('computer');
    expect(source.current(true)).toBe('computer');
  });

  it('falls back when the choice is no longer possible', () => {
    source.localAvailable = true;
    source.set('computer');
    // The computer was forgotten: the phone takes over.
    expect(source.current(false)).toBe('local');
    source.set('local');
    // A browser (no plugin): only the computer remains.
    source.localAvailable = false;
    expect(source.current(true)).toBe('computer');
    expect(source.current(false)).toBe('computer');
  });
});

describe('model sizes and progress', () => {
  it('formats gigabytes', () => {
    expect(gigabytes(2_588_147_712)).toBe('2.4 GB');
  });

  it('computes a percentage', () => {
    expect(percent(undefined)).toBe(0);
    expect(percent({ downloaded: 0, total: 0 })).toBe(0);
    expect(percent({ downloaded: 50, total: 200 })).toBe(25);
    expect(percent({ downloaded: 300, total: 200 })).toBe(100);
  });
});
