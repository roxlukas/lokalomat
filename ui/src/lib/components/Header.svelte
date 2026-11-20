<script lang="ts">
  import Search from '@lucide/svelte/icons/search';
  import MemoryStick from '@lucide/svelte/icons/memory-stick';
  import ListChecks from '@lucide/svelte/icons/list-checks';
  import { getLocale, t } from '../i18n/index.svelte';
  import { downloads, hardware } from '../stores.svelte';
  import { engines } from '../chat.svelte';
  import { jobs } from '../jobs.svelte';
  import { ipc } from '../ipc';
  import { settings } from '../settings.svelte';
  import { formatMb } from '../format';
  import ActivityTray from './ActivityTray.svelte';

  let { onOpenPalette }: { onOpenPalette: () => void } = $props();

  const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);
  const shortcut = isMac ? '⌘K' : 'Ctrl+K';
  let trayOpen = $state(false);
  let jobCount = $derived(
    downloads.pending.length + engines.list.filter((e) => e.state === 'installing').length + jobs.active.length,
  );
  let busy = $derived(
    downloads.running.length > 0 || engines.list.some((e) => e.state === 'installing') || jobs.active.length > 0,
  );
  let budget = $derived(hardware.view?.budget ?? null);
  // The VRAM scheduler's view: resident models and running jobs (ADR 0015).
  let loaded = $derived(jobs.memory?.items ?? []);
  let usedMb = $derived(jobs.memory?.used_mb ?? 0);
  let usedFraction = $derived(budget && budget.main_mb ? Math.min(1, usedMb / budget.main_mb) : 0);
  let memoryOpen = $state(false);
  let memoryTitle = $derived(
    budget
      ? t('header.memoryTitle', {
          used: formatMb(usedMb, getLocale()),
          budget: formatMb(budget.main_mb, getLocale()),
          kind: t(`fit.budgetKind.${budget.kind}`),
        })
      : t('header.memoryUnknown'),
  );

  async function release(key: string) {
    await ipc.releaseMemory(key).catch(() => undefined);
    memoryOpen = false;
  }
</script>

<header class="header">
  <div class="brand">
    <svg class="logo" viewBox="0 0 1024 1024" aria-hidden="true">
      <rect x="64" y="64" width="896" height="896" rx="200" fill="currentColor" />
      <path d="M352 272 V672 H672" fill="none" stroke="var(--surface)" stroke-width="96" stroke-linecap="round" stroke-linejoin="round" />
      <circle cx="672" cy="352" r="72" fill="var(--accent-contrast)" />
    </svg>
    <span class="name">{t('app.name')}</span>
  </div>

  <button type="button" class="palette-trigger" onclick={onOpenPalette} aria-keyshortcuts={isMac ? 'Meta+K' : 'Control+K'}>
    <Search size={16} aria-hidden="true" />
    <span class="placeholder">{t('header.palette')}</span>
    <kbd>{shortcut}</kbd>
  </button>

  <div class="meters">
    <div class="memory-wrap">
      <button type="button" class="memory" title={memoryTitle} aria-expanded={memoryOpen} aria-haspopup="dialog" onclick={() => (memoryOpen = !memoryOpen)}>
        <MemoryStick size={16} aria-hidden="true" />
        <span class="sr-only">{t('header.memory')}: {memoryTitle}</span>
        <span class="bar" aria-hidden="true"><span class="fill" class:high={usedFraction > 0.85} style:width="{Math.round(usedFraction * 100)}%"></span></span>
        <span class="value" aria-hidden="true">{formatMb(usedMb, getLocale())} / {budget ? formatMb(budget.main_mb, getLocale()) : '—'}</span>
      </button>
      {#if memoryOpen}
        <div class="memory-pop" role="dialog" aria-label={t('header.memory')}>
          <p class="pop-title">{memoryTitle}</p>
          {#if loaded.length === 0}
            <p class="pop-muted">{t('header.memoryEmpty')}</p>
          {:else}
            <ul>
              {#each loaded as m (m.key)}
                <li>
                  <span>
                    {m.name}<span class="pop-muted"> · {m.memory_mb ? formatMb(m.memory_mb, getLocale()) : t('header.cpuOnly')}{#if m.kind === 'job'} · {t('header.working')}{/if}{#if m.keep} · {t('header.kept')}{/if}</span>
                  </span>
                  <button type="button" class="btn small" onclick={() => release(m.key)}>
                    {m.kind === 'job' ? t('common.cancel') : t('header.release')}
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/if}
    </div>

    <div class="jobs">
      <button
        type="button"
        class="icon-btn"
        aria-label={t('header.jobs', { count: jobCount })}
        aria-expanded={trayOpen}
        aria-haspopup="dialog"
        onclick={() => (trayOpen = !trayOpen)}
      >
        <ListChecks size={18} aria-hidden="true" />
        {#if jobCount > 0}<span class="badge" class:pulse={busy}>{jobCount}</span>{/if}
      </button>
      {#if trayOpen}
        <ActivityTray onClose={() => (trayOpen = false)} />
      {/if}
    </div>

    <div class="mode" role="radiogroup" aria-label={t('header.mode.label')}>
      {#each [false, true] as expert (expert)}
        <button
          type="button"
          role="radio"
          aria-checked={settings.value.expert === expert}
          class:active={settings.value.expert === expert}
          title={t(expert ? 'header.mode.expertHint' : 'header.mode.basicHint')}
          onclick={() => settings.update({ expert }).catch(() => undefined)}
        >
          {t(expert ? 'header.mode.expert' : 'header.mode.basic')}
        </button>
      {/each}
    </div>
  </div>
</header>

<style>
  .header {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    height: var(--header-h);
    padding: 0 var(--space-4);
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--bg) 85%, transparent);
    backdrop-filter: blur(10px);
    /* backdrop-filter creates a stacking context: lift the whole header so the
       activity tray and memory popover open above the content. */
    position: relative;
    z-index: 40;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--accent);
    min-width: max-content;
  }

  .logo {
    width: 24px;
    height: 24px;
  }

  .name {
    font-family: var(--font-display);
    font-weight: 700;
    font-size: var(--text-l);
    color: var(--text);
  }

  .palette-trigger {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: 1;
    max-width: 520px;
    height: 34px;
    margin: 0 auto;
    padding: 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
    background: var(--surface);
    color: var(--text-muted);
    text-align: left;
  }

  .palette-trigger:hover {
    border-color: var(--border-strong);
  }

  .placeholder {
    flex: 1;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  kbd {
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }

  .meters {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .memory {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--text-muted);
    font-size: var(--text-s);
    font-variant-numeric: tabular-nums;
  }

  .bar {
    width: 72px;
    height: 6px;
    border-radius: 3px;
    background: var(--surface-3);
    overflow: hidden;
  }

  .fill {
    display: block;
    width: 0;
    height: 100%;
    background: var(--ok);
    transition: width var(--dur-mid) var(--ease);
  }

  .fill.high {
    background: var(--warn);
  }

  .memory-wrap {
    position: relative;
  }

  button.memory {
    padding: 4px var(--space-2);
    border: 0;
    border-radius: var(--radius-s);
    background: none;
  }

  button.memory:hover {
    background: var(--surface-2);
  }

  .memory-pop {
    position: absolute;
    right: 0;
    top: calc(100% + var(--space-2));
    width: min(340px, calc(100vw - 32px));
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-l);
    background: var(--surface);
    box-shadow: var(--shadow);
    font-size: var(--text-s);
    z-index: 30;
  }

  .memory-pop ul {
    display: grid;
    gap: var(--space-2);
    margin: var(--space-2) 0 0;
    padding: 0;
    list-style: none;
  }

  .memory-pop li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .pop-title {
    font-weight: 600;
  }

  .pop-muted {
    color: var(--text-muted);
  }

  .btn.small {
    min-height: 28px;
    padding: 0 var(--space-3);
    font-size: var(--text-s);
  }

  .jobs {
    position: relative;
  }

  .mode {
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
    background: var(--surface);
  }

  .mode button {
    padding: 3px 12px;
    border: 0;
    border-radius: var(--radius-pill);
    background: none;
    color: var(--text-muted);
    font-size: var(--text-s);
    font-weight: 600;
  }

  .mode button.active {
    background: var(--accent);
    color: var(--accent-contrast);
  }

  .badge {
    position: absolute;
    top: 2px;
    right: 2px;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    border-radius: 8px;
    background: var(--busy);
    color: var(--surface);
    font-size: 10px;
    line-height: 16px;
  }

  .badge.pulse {
    animation: pulse 1.6s var(--ease) infinite;
  }

  @keyframes pulse {
    50% {
      box-shadow: 0 0 0 4px color-mix(in srgb, var(--busy) 25%, transparent);
    }
  }

  @media (max-width: 720px) {
    .name,
    .memory .bar,
    kbd {
      display: none;
    }
  }

  @media (max-width: 480px) {
    .memory {
      display: none;
    }
  }
</style>
