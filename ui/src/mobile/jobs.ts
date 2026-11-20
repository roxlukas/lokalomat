/**
 * Jobs the phone starts on the computer (images, songs, decisions; ADR 0027).
 * The phone polls a job until it ends. The job id is kept on the phone, so a
 * screen that comes back (tab switch, app restart) picks the job up again.
 */
import { readLocal, writeLocal } from '../lib/storage';
import { api, finished, isOffline, type FileData, type JobView } from './api';

/** Poll interval while the app is on screen. */
export const POLL_MS = 1500;
/** Slower retries while the computer cannot be reached. */
export const OFFLINE_MS = 5000;

const key = (slot: string) => `lokalsi.mobile.job.${slot}`;

/** Remembers (or forgets, with null) the job shown on a screen. */
export function rememberJob(slot: string, jobId: number | null): void {
  writeLocal(key(slot), jobId);
}

export function rememberedJob(slot: string): number | null {
  const id = readLocal<number | null>(key(slot), null);
  return typeof id === 'number' && Number.isFinite(id) ? id : null;
}

export interface Watch {
  stop: () => void;
}

/**
 * Polls `jobId` until it ends: `onUpdate` gets every state, `onError` an error
 * that is not about the connection (the job is then left alone). Polling
 * pauses while the app is hidden.
 */
export function watchJob(
  jobId: number,
  onUpdate: (job: JobView) => void,
  onError: (e: unknown) => void,
  get: (id: number) => Promise<JobView> = api.job,
): Watch {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const visible = () => typeof document === 'undefined' || document.visibilityState === 'visible';

  async function tick() {
    if (stopped) return;
    let delay = POLL_MS;
    if (visible()) {
      try {
        const job = await get(jobId);
        if (stopped) return;
        onUpdate(job);
        if (finished(job)) return;
      } catch (e) {
        if (stopped) return;
        if (!isOffline(e)) {
          onError(e);
          return;
        }
        delay = OFFLINE_MS;
      }
    }
    timer = setTimeout(() => void tick(), delay);
  }

  void tick();
  return {
    stop: () => {
      stopped = true;
      if (timer) clearTimeout(timer);
    },
  };
}

/** The file to show: the first one of `type` (e.g. `audio/mpeg`), else the first. */
export function pickFile(job: JobView, type?: string): JobView['files'][number] | null {
  return job.files.find((f) => f.content_type === type) ?? job.files[0] ?? null;
}

const EXT: Record<string, string> = { 'image/png': 'png', 'audio/mpeg': 'mp3', 'audio/wav': 'wav' };

/** File name suggested in the save dialog, e.g. `lokalomat-obraz-42.png`. */
export function fileName(stem: string, job: JobView, file: { content_type: string }): string {
  const safe = stem.replace(/[^A-Za-z0-9_-]/g, '') || 'wynik';
  return `lokalomat-${safe}-${job.job_id}.${EXT[file.content_type] ?? 'bin'}`;
}

/** Downloads a result file of a job (through the phone's Rust side). */
export function download(job: JobView, index: number): Promise<FileData> {
  return api.galleryFile(job.job_id, index);
}
