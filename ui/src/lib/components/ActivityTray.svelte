<script lang="ts">
  import X from '@lucide/svelte/icons/x';
  import Pause from '@lucide/svelte/icons/pause';
  import Play from '@lucide/svelte/icons/play';
  import RotateCw from '@lucide/svelte/icons/rotate-cw';
  import ArrowUp from '@lucide/svelte/icons/arrow-up';
  import ArrowDown from '@lucide/svelte/icons/arrow-down';
  import { getLocale, t } from '../i18n/index.svelte';
  import { ipc, type DownloadItem } from '../ipc';
  import { downloads, models } from '../stores.svelte';
  import { engines } from '../chat.svelte';
  import { etaSeconds, formatBytes, formatDuration, formatSpeed } from '../format';
  import { describeDownloadError, describeError } from '../feedback';
  import ProgressBar from './ProgressBar.svelte';
  import { jobs } from '../jobs.svelte';
  import { router } from '../router.svelte';

  let { onClose }: { onClose: () => void } = $props();
  let panel: HTMLElement | undefined = $state();
  let error = $state('');

  let pending = $derived(downloads.pending);
  let finished = $derived(downloads.items.filter((i) => i.status === 'completed'));
  let installing = $derived(engines.list.filter((e) => e.state === 'installing'));
  let running = $derived(jobs.active);

  $effect(() => {
    panel?.focus();
    const onPointer = (e: PointerEvent) => {
      const target = e.target as Node;
      if (panel && !panel.contains(target) && !panel.parentElement?.contains(target)) onClose();
    };
    document.addEventListener('pointerdown', onPointer);
    return () => document.removeEventListener('pointerdown', onPointer);
  });

  function title(item: DownloadItem): string {
    const model = models.byId(item.model_id);
    const variant = model?.variants.find((v) => v.id === item.variant_id);
    return model ? `${model.name} · ${variant?.label ?? item.variant_id}` : item.model_id;
  }

  function fileName(item: DownloadItem): string {
    return item.file_path.split('/').pop() ?? item.file_path;
  }

  async function act(fn: () => Promise<unknown>) {
    error = '';
    try {
      await fn();
    } catch (e) {
      error = describeError(e);
    }
    await downloads.load();
  }

  function detail(item: DownloadItem): string {
    const loc = getLocale();
    const done = downloads.downloaded(item);
    const parts = [t('downloads.progress', { done: formatBytes(done, loc), total: formatBytes(item.size, loc) })];
    const speed = downloads.speed(item);
    if (item.status === 'downloading' && speed > 0) {
      parts.push(formatSpeed(speed, loc));
      const eta = etaSeconds(done, item.size, speed);
      if (eta !== null) parts.push(t('downloads.eta', { time: formatDuration(eta) }));
    }
    return parts.join(' · ');
  }
</script>

<div
  class="tray"
  role="dialog"
  aria-label={t('header.trayTitle')}
  tabindex="-1"
  bind:this={panel}
  onkeydown={(e) => e.key === 'Escape' && onClose()}
>
  <div class="head">
    <h2>{t('header.trayTitle')}</h2>
    <button type="button" class="icon-btn" aria-label={t('common.close')} onclick={onClose}><X size={16} aria-hidden="true" /></button>
  </div>

  {#if error}<p class="note note-error" role="alert">{error}</p>{/if}

  {#each installing as e (e.id)}
    {@const p = e.install_progress}
    <div class="item">
      <div class="line">
        <span class="name">{t('downloads.engine', { name: e.name })}</span>
        <span class="status status-downloading">{t('downloads.status.downloading')}</span>
      </div>
      <ProgressBar value={p && p[1] ? p[0] / p[1] : null} label={t('downloads.engine', { name: e.name })} />
      {#if p}<span class="detail">{t('downloads.progress', { done: formatBytes(p[0], getLocale()), total: formatBytes(p[1], getLocale()) })}</span>{/if}
    </div>
  {/each}

  {#each running as j (j.id)}
    {@const label = j.kind === 'image' ? t('workshop.modes.image') : t('workshop.modes.stt')}
    <div class="item">
      <div class="line">
        <button type="button" class="link name" onclick={() => { router.go('workshop'); onClose(); }}>{label} · {j.model_name}</button>
        <span class="status status-downloading">{t(`jobs.status.${j.status}`)}</span>
      </div>
      <div class="line">
        <ProgressBar value={j.status === 'running' ? j.progress : null} label={label} />
        <button type="button" class="icon-btn sm" aria-label={t('common.cancel')} title={t('common.cancel')}
          onclick={() => act(() => jobs.cancel(j.id))}><X size={14} aria-hidden="true" /></button>
      </div>
    </div>
  {/each}

  {#if pending.length === 0 && finished.length === 0 && installing.length === 0 && running.length === 0}
    <p class="empty">{t('header.trayEmpty')}</p>
  {:else}
    <ul class="items" aria-label={t('downloads.sectionActive')}>
      {#each pending as item, index (item.id)}
        {@const verifying = downloads.verifying(item)}
        <li class="item">
          <div class="line">
            <span class="name" title={item.dest}>{title(item)}</span>
            <span class="status status-{item.status}">
              {verifying ? t('downloads.status.verifying') : t(`downloads.status.${item.status}`)}
            </span>
          </div>
          <span class="file">{fileName(item)}</span>
          <ProgressBar
            value={verifying ? null : item.size ? downloads.downloaded(item) / item.size : null}
            label={t('models.row.progressLabel', { name: title(item) })}
            tone={item.status === 'failed' ? 'error' : item.status === 'paused' ? 'muted' : 'busy'}
          />
          <div class="line">
            <span class="detail">
              {#if item.status === 'failed'}
                <span class="err">{describeDownloadError(item.error)}</span>
              {:else if item.status === 'queued' && item.error === 'checksum_mismatch'}
                {t('downloads.retryingChecksum')}
              {:else}
                {detail(item)}
              {/if}
            </span>
            <span class="actions">
              {#if item.status === 'queued'}
                <button type="button" class="icon-btn sm" aria-label={t('downloads.moveUp')} title={t('downloads.moveUp')}
                  disabled={index === 0} onclick={() => act(() => ipc.moveDownload(item.id, true))}><ArrowUp size={14} aria-hidden="true" /></button>
                <button type="button" class="icon-btn sm" aria-label={t('downloads.moveDown')} title={t('downloads.moveDown')}
                  disabled={index === pending.length - 1} onclick={() => act(() => ipc.moveDownload(item.id, false))}><ArrowDown size={14} aria-hidden="true" /></button>
              {/if}
              {#if item.status === 'downloading' || item.status === 'queued' || item.status === 'verifying'}
                <button type="button" class="icon-btn sm" aria-label={t('downloads.pause')} title={t('downloads.pause')}
                  onclick={() => act(() => ipc.pauseDownload(item.id))}><Pause size={14} aria-hidden="true" /></button>
              {:else if item.status === 'paused'}
                <button type="button" class="icon-btn sm" aria-label={t('downloads.resume')} title={t('downloads.resume')}
                  onclick={() => act(() => ipc.resumeDownload(item.id))}><Play size={14} aria-hidden="true" /></button>
              {:else if item.status === 'failed'}
                <button type="button" class="icon-btn sm" aria-label={t('downloads.retry')} title={t('downloads.retry')}
                  onclick={() => act(() => ipc.resumeDownload(item.id))}><RotateCw size={14} aria-hidden="true" /></button>
              {/if}
              <button type="button" class="icon-btn sm" aria-label={t('downloads.cancelModel', { name: title(item) })} title={t('downloads.cancelModel', { name: title(item) })}
                onclick={() => act(() => ipc.cancelVariantDownload(item.model_id, item.variant_id))}><X size={14} aria-hidden="true" /></button>
            </span>
          </div>
        </li>
      {/each}
    </ul>

    {#if finished.length}
      <div class="done-head">
        <h3>{t('downloads.sectionDone')}</h3>
        <button type="button" class="link" onclick={() => act(() => ipc.clearFinishedDownloads())}>{t('downloads.clear')}</button>
      </div>
      <ul class="items done">
        {#each finished as item (item.id)}
          <li class="item">
            <div class="line">
              <span class="name">{title(item)}</span>
              <span class="status status-completed">{t('downloads.status.completed')}</span>
            </div>
            <span class="file">{fileName(item)} · {formatBytes(item.size, getLocale())}</span>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</div>

<style>
  .tray {
    position: absolute;
    right: 0;
    top: calc(100% + var(--space-2));
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    width: min(420px, calc(100vw - 32px));
    max-height: min(70vh, 640px);
    padding: var(--space-3) var(--space-4) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-l);
    background: var(--surface);
    box-shadow: var(--shadow);
    overflow-y: auto;
    z-index: 30;
  }

  .tray:focus {
    outline: none;
  }

  .head,
  .done-head,
  .line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  h2 {
    font-size: var(--text-l);
  }

  h3 {
    margin: var(--space-2) 0 0;
    font-size: var(--text-s);
    color: var(--text-muted);
  }

  .empty {
    color: var(--text-muted);
    font-size: var(--text-s);
  }

  .items {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .item {
    display: grid;
    gap: 4px;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
  }

  .done .item {
    border-style: dashed;
  }

  .name {
    min-width: 0;
    overflow: hidden;
    font-weight: 600;
    font-size: var(--text-s);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .file,
  .detail {
    color: var(--text-muted);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    overflow-wrap: anywhere;
  }

  .status {
    flex: none;
    font-size: var(--text-xs);
    color: var(--busy);
  }

  .status-paused,
  .status-queued {
    color: var(--text-muted);
  }

  .status-failed,
  .err {
    color: var(--error);
  }

  .status-completed {
    color: var(--ok);
  }

  .actions {
    display: flex;
    flex: none;
  }

  .icon-btn.sm {
    width: 28px;
    height: 28px;
  }

  .icon-btn.sm:disabled {
    opacity: 0.35;
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--focus);
    font-size: var(--text-s);
    text-decoration: underline;
  }
</style>
