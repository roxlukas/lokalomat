import { describe, expect, it } from 'vitest';
import { diagnosticsFileName, etaSeconds, formatBytes, formatDuration, formatMb } from './format';

describe('formatBytes', () => {
  it('formats with units', () => {
    expect(formatBytes(512, 'en')).toBe('512 B');
    expect(formatBytes(1536, 'en')).toBe('1.5 KB');
    expect(formatBytes(5 * 1024 * 1024 * 1024, 'en')).toBe('5.0 GB');
    expect(formatMb(24064, 'en')).toBe('23.5 GB');
  });
});

describe('eta', () => {
  it('estimates remaining time', () => {
    expect(etaSeconds(50, 150, 10)).toBe(10);
    expect(etaSeconds(150, 150, 10)).toBeNull();
    expect(etaSeconds(0, 150, 0)).toBeNull();
    expect(formatDuration(45)).toBe('45 s');
    expect(formatDuration(720)).toBe('12 min');
    expect(formatDuration(4800)).toBe('1 h 20 min');
  });
});

describe('diagnosticsFileName', () => {
  it('stamps date and time', () => {
    expect(diagnosticsFileName(new Date(2026, 10, 11, 9, 5))).toBe('lokalomat-diagnostics-20261111-0905.zip');
  });
});
