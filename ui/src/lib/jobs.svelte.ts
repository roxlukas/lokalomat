/** Jobs (decisions here, media jobs in the Pro edition) and the memory meter, fed by IPC and events. */
import {
  EVENTS,
  errorMessage,
  ipc,
  onEvent,
  type Job,
  type JobKind,
  type JobProgress,
  type MemoryStatus,
  type DecisionForm,
} from './ipc';
import { jobFinished } from './media';
import { logToApp } from './errors';
import { t } from './i18n/index.svelte';
import { storedErrorParams } from './feedback';

export { jobFinished, jobImages, jobSeeds, jobSegments, segmentAt, clock, modelOptions, jobAudio } from './media';

/** Translated job error stored as `code` or `code: details`. */
export function describeJobError(error: string | null): string {
  if (!error) return '';
  const code = error.split(':')[0]!.trim();
  const key = `jobs.errors.${code}`;
  const text = t(key, storedErrorParams(error));
  return text === key ? t('jobs.errors.engine_error') : text;
}

class JobsStore {
  /** Recent jobs, newest last (Workshop timeline). */
  list = $state<Job[]>([]);
  memory = $state<MemoryStatus | null>(null);
  /** Parameters to put back into the Workshop ("Odtwórz parametry"). */
  replay = $state<{ kind: JobKind; params: Record<string, unknown> } | null>(null);

  active = $derived(this.list.filter((j) => !jobFinished(j)));

  of(kind: JobKind): Job[] {
    return this.list.filter((j) => j.kind === kind);
  }

  async load(): Promise<void> {
    try {
      const recent = await ipc.listJobs({ limit: 100 });
      this.list = recent.reverse();
    } catch (e) {
      logToApp('warn', `list_jobs failed: ${errorMessage(e)}`);
    }
  }

  async loadMemory(): Promise<void> {
    try {
      this.memory = await ipc.getMemory();
    } catch (e) {
      logToApp('warn', `get_memory failed: ${errorMessage(e)}`);
    }
  }

  /** Adds a job or replaces the one with the same id (also used by the Pro job starters). */
  upsert(job: Job): void {
    const i = this.list.findIndex((j) => j.id === job.id);
    if (i >= 0) this.list[i] = job;
    else this.list.push(job);
  }

  async decide(form: DecisionForm): Promise<void> {
    this.upsert(await ipc.decide(form));
  }

  async cancel(id: number): Promise<void> {
    await ipc.cancelJob(id);
  }

  async remove(ids: number[]): Promise<void> {
    await ipc.deleteJobs(ids);
    this.list = this.list.filter((j) => !ids.includes(j.id));
  }

  async listen(): Promise<void> {
    await onEvent<Job>(EVENTS.jobsUpdated, (job) => this.upsert(job));
    await onEvent<JobProgress>(EVENTS.jobsProgress, (p) => {
      const job = this.list.find((j) => j.id === p.job_id);
      if (job) job.progress = p.progress;
    });
    await onEvent<MemoryStatus>(EVENTS.memoryChanged, (m) => (this.memory = m));
  }
}

export const jobs = new JobsStore();

export async function initJobs(): Promise<void> {
  await jobs.listen();
  await Promise.all([jobs.load(), jobs.loadMemory()]);
}
