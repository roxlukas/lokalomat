/** Reactive stores fed by IPC commands and backend events. */
import {
  errorMessage,
  EVENTS,
  ipc,
  onEvent,
  type BenchmarkMeta,
  type DownloadItem,
  type HardwareView,
  type ModelView,
  type Progress,
} from './ipc';
import { logToApp } from './errors';

class HardwareStore {
  view = $state<HardwareView | null>(null);
  probing = $state(false);
  error = $state('');

  async load(refresh = false): Promise<void> {
    this.probing = true;
    this.error = '';
    try {
      this.view = await ipc.getHardware(refresh);
    } catch (e) {
      this.error = errorMessage(e);
      logToApp('warn', `hardware probe failed: ${this.error}`);
    } finally {
      this.probing = false;
    }
  }

  async listen(): Promise<void> {
    await onEvent<HardwareView>(EVENTS.hardwareUpdated, (view) => {
      this.view = view;
      void models.load();
    });
  }
}

class ModelsStore {
  list = $state<ModelView[]>([]);
  /** AI Transformation benchmark; `null` while loading or when the site did not respond. */
  bench = $state<BenchmarkMeta | null>(null);
  loaded = $state(false);
  error = $state('');
  private pending: ReturnType<typeof setTimeout> | null = null;

  async loadBenchmark(): Promise<void> {
    this.bench = await ipc.getBenchmarkMeta().catch(() => null);
  }

  async load(): Promise<void> {
    try {
      this.list = await ipc.listModels();
      this.error = '';
    } catch (e) {
      this.error = errorMessage(e);
    } finally {
      this.loaded = true;
    }
  }

  /** Coalesces bursts of backend events into one reload. */
  reloadSoon(): void {
    if (this.pending) return;
    this.pending = setTimeout(() => {
      this.pending = null;
      void this.load();
    }, 150);
  }

  byId(id: string): ModelView | undefined {
    return this.list.find((m) => m.id === id);
  }

  async listen(): Promise<void> {
    await onEvent(EVENTS.modelInstalled, () => this.reloadSoon());
    await onEvent(EVENTS.downloadsChanged, () => this.reloadSoon());
    await onEvent<BenchmarkMeta>(EVENTS.benchmarkUpdated, (meta) => {
      this.bench = meta;
      this.reloadSoon();
    });
  }
}

const ACTIVE: DownloadItem['status'][] = ['queued', 'downloading', 'verifying', 'paused', 'failed'];

class DownloadsStore {
  items = $state<DownloadItem[]>([]);
  progress = $state<Record<number, Progress>>({});
  /** Bumps when a variant finishes, so views can show a short confirmation. */
  lastInstalled = $state<{ model_id: string; variant_id: string } | null>(null);

  pending = $derived(this.items.filter((i) => ACTIVE.includes(i.status)));
  running = $derived(this.items.filter((i) => i.status === 'downloading' || i.status === 'verifying'));

  async load(): Promise<void> {
    try {
      this.items = await ipc.listDownloads();
    } catch (e) {
      logToApp('warn', `list downloads failed: ${errorMessage(e)}`);
    }
  }

  async listen(): Promise<void> {
    await onEvent(EVENTS.downloadsChanged, () => void this.load());
    await onEvent<Progress>(EVENTS.downloadsProgress, (p) => {
      this.progress[p.id] = p;
    });
    await onEvent<{ model_id: string; variant_id: string }>(EVENTS.modelInstalled, (m) => {
      this.lastInstalled = m;
    });
  }

  /** Live bytes for an item: progress events are fresher than the list. */
  downloaded(item: DownloadItem): number {
    return this.progress[item.id]?.downloaded ?? item.downloaded;
  }

  speed(item: DownloadItem): number {
    const p = this.progress[item.id];
    return p && p.status === 'downloading' ? p.bytes_per_sec : 0;
  }

  verifying(item: DownloadItem): boolean {
    return item.status === 'verifying' || this.progress[item.id]?.status === 'verifying';
  }
}

export const hardware = new HardwareStore();
export const models = new ModelsStore();
export const downloads = new DownloadsStore();

/** Loads everything and subscribes to backend events (called once at startup). */
export async function initStores(): Promise<void> {
  await Promise.all([hardware.listen(), models.listen(), downloads.listen()]);
  await Promise.all([hardware.load(), models.load(), models.loadBenchmark(), downloads.load()]);
}
