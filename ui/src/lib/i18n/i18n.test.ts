import { describe, expect, it } from 'vitest';
import pl from '../../../locales/pl.json';
import en from '../../../locales/en.json';
import { flattenKeys, interpolate, lookup } from './translate';

describe('locales', () => {
  it('pl and en define exactly the same keys', () => {
    expect(flattenKeys(en)).toEqual(flattenKeys(pl));
  });

  it('no translation is empty', () => {
    for (const dict of [pl, en]) {
      for (const key of flattenKeys(dict)) {
        expect(lookup(dict, key)?.trim(), key).toBeTruthy();
      }
    }
  });

  // PRD §15: explanations are short, plain sentences.
  it('texts stay short', () => {
    for (const dict of [pl, en]) {
      for (const key of flattenKeys(dict)) {
        expect(lookup(dict, key)!.length, key).toBeLessThanOrEqual(90);
      }
    }
  });

  it('placeholders match between languages', () => {
    const names = (s: string | undefined) => [...(s ?? '').matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
    for (const key of flattenKeys(pl)) {
      expect(names(lookup(en, key)), key).toEqual(names(lookup(pl, key)));
    }
  });
});

describe('translate helpers', () => {
  it('looks up nested keys', () => {
    expect(lookup({ a: { b: 'x' } }, 'a.b')).toBe('x');
    expect(lookup({ a: { b: 'x' } }, 'a')).toBeUndefined();
    expect(lookup({ a: 'x' }, 'a.b')).toBeUndefined();
  });

  it('interpolates known params and keeps unknown placeholders', () => {
    expect(interpolate('{count} z {total}', { count: 2 })).toBe('2 z {total}');
  });
});
