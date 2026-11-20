/** Pure helpers for transcription, image and speech results (tested in media.test.ts). */
import type { Job, ModelView, Segment } from './ipc';

const FINISHED = new Set(['done', 'failed', 'cancelled']);

export function jobFinished(job: Job): boolean {
  return FINISHED.has(job.status);
}

/** Image files of a finished image job. */
export function jobImages(job: Job): string[] {
  const files = job.outputs['files'];
  return Array.isArray(files) ? files.filter((f): f is string => typeof f === 'string') : [];
}

export function jobSeeds(job: Job): number[] {
  const seeds = job.outputs['seeds'];
  return Array.isArray(seeds) ? seeds.filter((s): s is number => typeof s === 'number') : [];
}

export function jobSegments(job: Job): Segment[] {
  const segs = job.outputs['segments'];
  return Array.isArray(segs) ? (segs as Segment[]) : [];
}

/** Index of the segment playing at `ms`, or -1 (segments sorted by time). */
export function segmentAt(segments: Segment[], ms: number): number {
  let lo = 0;
  let hi = segments.length - 1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    const s = segments[mid]!;
    if (ms < s.start_ms) hi = mid - 1;
    else if (ms >= s.end_ms) lo = mid + 1;
    else return mid;
  }
  return -1;
}

/** `mm:ss` or `h:mm:ss`. */
export function clock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, '0');
  return h ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

/** Plain text of a transcript, one segment per line. */
export function transcriptText(segments: Segment[]): string {
  return segments
    .map((s) => s.text.trim())
    .filter(Boolean)
    .join('\n');
}

/** Installed variants of a modality as `{ modelId, variantId, label, cloning }`. */
export function modelOptions(models: ModelView[], modality: 'image' | 'stt' | 'tts' | 'music' | 'decision') {
  return models
    .filter((m) => m.modality === modality)
    .flatMap((m) =>
      m.variants
        .filter((v) => v.state === 'installed')
        .map((v) => ({
          modelId: m.id,
          variantId: v.id,
          label: `${m.name} · ${v.label}`,
          cloning: m.features.includes('voice_cloning'),
          speed: v.speed ?? null,
        })),
    );
}

/** Audio file of a finished speech or song job: `wav` (default) or `mp3`. */
export function jobAudio(job: Job, ext: 'wav' | 'mp3' = 'wav'): string | null {
  const files = job.outputs['files'];
  if (!Array.isArray(files)) return null;
  const hit = files.find((f): f is string => typeof f === 'string' && f.toLowerCase().endsWith(`.${ext}`));
  return hit ?? null;
}

/** 16-bit mono PCM WAV of `samples` (−1…1), for microphone recordings. */
export function encodeWav(samples: Float32Array, rate: number): Uint8Array {
  const data = samples.length * 2;
  const buf = new ArrayBuffer(44 + data);
  const v = new DataView(buf);
  const text = (at: number, s: string) => [...s].forEach((c, i) => v.setUint8(at + i, c.charCodeAt(0)));
  text(0, 'RIFF');
  v.setUint32(4, 36 + data, true);
  text(8, 'WAVEfmt ');
  v.setUint32(16, 16, true);
  v.setUint16(20, 1, true);
  v.setUint16(22, 1, true);
  v.setUint32(24, rate, true);
  v.setUint32(28, rate * 2, true);
  v.setUint16(32, 2, true);
  v.setUint16(34, 16, true);
  text(36, 'data');
  v.setUint32(40, data, true);
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]!));
    v.setInt16(44 + i * 2, Math.round(s * 32767), true);
  }
  return new Uint8Array(buf);
}

/** Mixes channels of a decoded recording to mono. */
export function toMono(channels: Float32Array[]): Float32Array {
  if (channels.length === 1) return channels[0]!;
  const n = channels[0]?.length ?? 0;
  const out = new Float32Array(n);
  for (const ch of channels) for (let i = 0; i < n; i++) out[i]! += ch[i]! / channels.length;
  return out;
}

/** Audio files accepted for transcription. */
export const AUDIO_EXTENSIONS = ['wav', 'mp3', 'flac', 'ogg', 'oga', 'm4a', 'mp4', 'aac', 'mka', 'mkv', 'aiff', 'caf'];

export function isAudioPath(path: string): boolean {
  const ext = path.split('.').pop()?.toLowerCase() ?? '';
  return AUDIO_EXTENSIONS.includes(ext);
}

export function fileNameOf(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
