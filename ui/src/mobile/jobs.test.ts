import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { JobView } from './api';
import { fileName, OFFLINE_MS, pickFile, POLL_MS, watchJob } from './jobs';

function job(status: JobView['status'], extra: Partial<JobView> = {}): JobView {
  return { job_id: 42, kind: 'music', status, progress: null, error: null, model: 'ACE-Step', files: [], answers: [], ...extra };
}

describe('phone jobs (ADR 0027)', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('polls until the job ends', async () => {
    const states = [job('queued'), job('running', { progress: 0.5 }), job('done')];
    const get = vi.fn(async () => states.shift()!);
    const seen: string[] = [];
    watchJob(42, (j) => seen.push(j.status), () => undefined, get);
    await vi.advanceTimersByTimeAsync(POLL_MS * 5);
    expect(seen).toEqual(['queued', 'running', 'done']);
    expect(get).toHaveBeenCalledTimes(3);
  });

  it('keeps trying slower while the computer is away, and stops on other errors', async () => {
    let calls = 0;
    const get = vi.fn(async () => {
      calls += 1;
      if (calls === 1) throw { code: 'unreachable', message: '' };
      if (calls === 2) return job('running');
      throw { code: 'not_found', message: '' };
    });
    const errors: unknown[] = [];
    watchJob(42, () => undefined, (e) => errors.push(e), get);
    // Offline: no retry after the usual interval, one after the slower one.
    await vi.advanceTimersByTimeAsync(POLL_MS);
    expect(calls).toBe(1);
    await vi.advanceTimersByTimeAsync(OFFLINE_MS - POLL_MS);
    expect(calls).toBe(2);
    await vi.advanceTimersByTimeAsync(POLL_MS * 4);
    expect(calls).toBe(3);
    expect(errors).toEqual([{ code: 'not_found', message: '' }]);
  });

  it('stops when asked', async () => {
    const get = vi.fn(async () => job('running'));
    const w = watchJob(42, () => undefined, () => undefined, get);
    await vi.advanceTimersByTimeAsync(0);
    w.stop();
    await vi.advanceTimersByTimeAsync(POLL_MS * 5);
    expect(get).toHaveBeenCalledTimes(1);
  });

  it('prefers the MP3 of a song and names files safely', () => {
    const song = job('done', {
      files: [
        { index: 0, content_type: 'audio/wav' },
        { index: 1, content_type: 'audio/mpeg' },
      ],
    });
    expect(pickFile(song, 'audio/mpeg')?.index).toBe(1);
    expect(pickFile(job('done', { files: [{ index: 0, content_type: 'audio/wav' }] }), 'audio/mpeg')?.index).toBe(0);
    expect(pickFile(job('done'))).toBeNull();
    expect(fileName('piosenka', song, song.files[1]!)).toBe('lokalomat-piosenka-42.mp3');
    expect(fileName('../ź', song, { content_type: 'x/y' })).toBe('lokalomat-wynik-42.bin');
  });
});
