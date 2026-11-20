import { describe, expect, it } from 'vitest';
import type { Job, ModelView } from './ipc';
import { clock, encodeWav, isAudioPath, fileNameOf, jobAudio, jobImages, jobSegments, modelOptions, segmentAt, toMono, transcriptText } from './media';

const segs = [
  { start_ms: 0, end_ms: 2000, text: ' Dzień dobry.' },
  { start_ms: 2000, end_ms: 5000, text: 'Witamy.' },
  { start_ms: 6000, end_ms: 9000, text: '' },
];

function job(outputs: Record<string, unknown>): Job {
  return {
    id: 1,
    kind: 'image',
    status: 'done',
    progress: 1,
    model_id: 'm',
    variant_id: 'v',
    model_name: 'M',
    engine: 'sd-cpp',
    params: {},
    inputs: {},
    outputs,
    text: '',
    error: null,
    created_at: 0,
    started_at: null,
    finished_at: null,
    duration_ms: null,
  };
}

describe('transcript helpers', () => {
  it('finds the segment playing now', () => {
    expect(segmentAt(segs, 0)).toBe(0);
    expect(segmentAt(segs, 1999)).toBe(0);
    expect(segmentAt(segs, 2000)).toBe(1);
    expect(segmentAt(segs, 5500)).toBe(-1);
    expect(segmentAt([], 10)).toBe(-1);
  });

  it('formats clock times and text', () => {
    expect(clock(0)).toBe('00:00');
    expect(clock(65_400)).toBe('01:05');
    expect(clock(3_725_000)).toBe('1:02:05');
    expect(transcriptText(segs)).toBe('Dzień dobry.\nWitamy.');
  });

  it('reads job outputs defensively', () => {
    expect(jobImages(job({ files: ['a.png', 3, 'b.png'] }))).toEqual(['a.png', 'b.png']);
    expect(jobImages(job({}))).toEqual([]);
    expect(jobSegments(job({ segments: segs }))).toHaveLength(3);
    expect(jobSegments(job({ segments: 'x' }))).toEqual([]);
  });

  it('recognises audio files and names', () => {
    expect(isAudioPath('C:\\nagrania\\spotkanie.MP3')).toBe(true);
    expect(isAudioPath('/home/a/notes.txt')).toBe(false);
    expect(fileNameOf('C:\\nagrania\\spotkanie.mp3')).toBe('spotkanie.mp3');
    expect(fileNameOf('/home/a/b.wav')).toBe('b.wav');
  });

  it('lists installed variants of a modality', () => {
    const models = [
      { id: 'w', name: 'Whisper', modality: 'stt', features: [], variants: [{ id: 'q5', label: 'Q5', state: 'installed' }, { id: 'f16', label: 'F16', state: 'not_installed' }] },
      { id: 'z', name: 'Z', modality: 'image', features: [], variants: [{ id: 'q4', label: 'Q4', state: 'installed' }] },
    ] as unknown as ModelView[];
    expect(modelOptions(models, 'stt')).toEqual([
      { modelId: 'w', variantId: 'q5', label: 'Whisper · Q5', cloning: false, speed: null },
    ]);
    const tts = [
      { id: 'c', name: 'Chatterbox', modality: 'tts', features: ['voice_cloning'], variants: [{ id: 'v3', label: 'v3', state: 'installed' }] },
    ] as unknown as ModelView[];
    expect(modelOptions(tts, 'tts')[0]?.cloning).toBe(true);
  });
});

describe('speech helpers', () => {
  it('finds the WAV and MP3 of a speech job', () => {
    const j = job({ files: ['C:/o/tts-1.wav', 'C:/o/tts-1.mp3'] });
    expect(jobAudio(j)).toBe('C:/o/tts-1.wav');
    expect(jobAudio(j, 'mp3')).toBe('C:/o/tts-1.mp3');
    expect(jobAudio(job({}))).toBeNull();
  });

  it('encodes a mono 16-bit WAV', () => {
    const wav = encodeWav(new Float32Array([0, 1, -1, 2]), 24000);
    const v = new DataView(wav.buffer);
    expect(String.fromCharCode(...wav.slice(0, 4))).toBe('RIFF');
    expect(v.getUint32(24, true)).toBe(24000);
    expect(wav.length).toBe(44 + 8);
    expect([v.getInt16(44, true), v.getInt16(46, true), v.getInt16(48, true), v.getInt16(50, true)]).toEqual([0, 32767, -32767, 32767]);
  });

  it('mixes stereo to mono', () => {
    expect(Array.from(toMono([new Float32Array([1, 0]), new Float32Array([0, 0])]))).toEqual([0.5, 0]);
  });
});
