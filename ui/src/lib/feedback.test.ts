import { describe, expect, it } from 'vitest';
import { describeError, storedErrorParams } from './feedback';
import { describeJobError } from './jobs.svelte';

describe('missing system library (ADR 0017)', () => {
  it('takes the package from a stored error', () => {
    expect(storedErrorParams('engine_missing_library: libgomp1')).toEqual({ package: 'libgomp1' });
    expect(storedErrorParams('engine_crashed')).toEqual({ package: '' });
  });

  it('shows the install command', () => {
    expect(describeJobError('engine_missing_library: libgomp1')).toContain('sudo apt install libgomp1');
    expect(describeError({ code: 'engine_missing_library', message: 'libgomp1' })).toContain(
      'sudo apt install libgomp1',
    );
  });
});
