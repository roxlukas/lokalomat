import { describe, expect, it } from 'vitest';
import { qrPath } from './qr';

describe('qrPath', () => {
  it('merges runs of dark modules and adds the quiet zone', () => {
    // 3×3: row 0 = 110, row 1 = 000, row 2 = 101
    expect(qrPath(3, '110000101', 4)).toBe('M4 4h2v1h-2zM4 6h1v1h-1zM6 6h1v1h-1z');
    expect(qrPath(2, '0000')).toBe('');
  });
});
