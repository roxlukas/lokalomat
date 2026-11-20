import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { REMIND_AFTER_MS, closeBanner, isBannerClosed } from './proBanner';

function stubStorage(data: Record<string, string> = {}) {
  vi.stubGlobal('localStorage', {
    getItem: (k: string) => data[k] ?? null,
    setItem: (k: string, v: string) => void (data[k] = v),
  });
  return data;
}

describe('Pro banner', () => {
  beforeEach(() => stubStorage());
  afterEach(() => vi.unstubAllGlobals());

  it('is open by default', () => {
    expect(isBannerClosed()).toBe(false);
  });

  it('stays closed until the reminder time passes', () => {
    closeBanner(1_000);
    expect(isBannerClosed(1_000 + REMIND_AFTER_MS - 1)).toBe(true);
    expect(isBannerClosed(1_000 + REMIND_AFTER_MS)).toBe(false);
  });

  it('does not throw when storage is blocked', () => {
    vi.stubGlobal('localStorage', {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    });
    expect(isBannerClosed()).toBe(false);
    expect(() => closeBanner()).not.toThrow();
  });
});
