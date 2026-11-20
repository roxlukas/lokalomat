<script lang="ts">
  import Cpu from '@lucide/svelte/icons/cpu';
  import Download from '@lucide/svelte/icons/download';
  import RotateCw from '@lucide/svelte/icons/rotate-cw';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import ScrollText from '@lucide/svelte/icons/scroll-text';
  import X from '@lucide/svelte/icons/x';
  import Wrench from '@lucide/svelte/icons/wrench';
  import { getLocale, t } from '../lib/i18n/index.svelte';
  import { inTauri, ipc, type EngineStatus } from '../lib/ipc';
  import { engines } from '../lib/chat.svelte';
  import { formatBytes } from '../lib/format';
  import { describeError } from '../lib/feedback';
  import ProgressBar from '../lib/components/ProgressBar.svelte';

  /** Engines planned for later milestones (shown so the view reflects PRD V5). */
  const UPCOMING: { id: string; name: string; caps: string; milestone: string }[] = [];

  let build = $state<Record<string, string>>({});
  let confirmUninstall = $state<string | null>(null);
  let confirmRepair = $state<string | null>(null);
  let logs = $state<Record<string, string | null>>({});
  let error = $state('');

  async function act(fn: () => Promise<unknown>) {
    error = '';
    try {
      await fn();
    } catch (e) {
      error = describeError(e);
    }
  }

  function chosenBuild(e: EngineStatus): string | undefined {
    return build[e.id] ?? e.builds.find((b) => b.recommended)?.id;
  }

  async function toggleLogs(e: EngineStatus) {
    if (logs[e.id] != null) {
      logs[e.id] = null;
      return;
    }
    logs[e.id] = await ipc.getEngineLog(e.id, 200).catch((err) => describeError(err));
  }

  function backendLabel(b: string): string {
    return b === 'cpu' ? 'CPU' : b === 'cuda' ? 'CUDA' : b === 'metal' ? 'Metal' : b === 'vulkan' ? 'Vulkan' : b;
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex (the view scrolls and may have no buttons) -->
<div class="view" role="region" tabindex="0" aria-labelledby="engines-title">
  <h1 id="engines-title">{t('views.engines.title')}</h1>
  <p class="muted">{t('engines.intro')}</p>
  {#if error}<p class="note note-error" role="alert">{error}</p>{/if}
  {#if !inTauri}<p class="note note-info">{t('settings.desktopOnly')}</p>{/if}

  {#each engines.list as e (e.id)}
    {@const progress = e.install_progress}
    <article class="card">
      <header>
        <Cpu size={20} aria-hidden="true" />
        <div class="title">
          <h2>{e.name}</h2>
          <span class="muted small">{e.capabilities.map((c) => t(`engines.capabilities.${c}`)).join(', ')} · {e.version} · {e.license}</span>
        </div>
        <span class="state state-{e.state}">{t(`engines.state.${e.state}`)}</span>
      </header>

      <dl class="facts">
        {#if e.installed}
          <div><dt>{t('engines.backend')}</dt><dd>{backendLabel(e.installed.backend)} ({e.installed.build_id})</dd></div>
          <div><dt>{t('engines.installedVersion')}</dt><dd>{e.installed.version}{#if e.update_available} · <strong>{t('engines.updateAvailable', { version: e.version })}</strong>{/if}</dd></div>
        {/if}
        {#if e.kind === 'python' && e.disk_size}
          <div><dt>{t('engines.diskSize')}</dt><dd>{formatBytes(e.disk_size, getLocale())}</dd></div>
        {/if}
        {#if e.loaded}
          <div><dt>{t('engines.loaded')}</dt><dd>{e.loaded.name}</dd></div>
        {/if}
      </dl>

      {#if e.error}<p class="note note-error">{t('engines.lastError', { error: e.error })}</p>{/if}

      {#if e.state === 'installing' && progress}
        <div class="progress">
          <ProgressBar value={progress[1] ? progress[0] / progress[1] : null} label={t('engines.installing', { name: e.name })} />
          {#if e.kind === 'python'}
            <span class="muted small">{t(`engines.stage.${e.stage ?? 'packages'}`)} · {Math.round((progress[0] / (progress[1] || 1)) * 100)}%</span>
          {:else}
            <span class="muted small">{formatBytes(progress[0], getLocale())} / {formatBytes(progress[1], getLocale())}</span>
          {/if}
          <button type="button" class="btn small" onclick={() => act(() => ipc.cancelEngineInstall(e.id))}><X size={14} aria-hidden="true" />{t('common.cancel')}</button>
        </div>
      {:else}
        <div class="actions">
          {#if !e.installed || e.update_available}
            {#if e.builds.length > 1}
              <label class="build">
                <span class="sr-only">{t('engines.chooseBuild')}</span>
                <select value={chosenBuild(e)} onchange={(ev) => (build[e.id] = ev.currentTarget.value)}>
                  {#each e.builds as b (b.id)}
                    <option value={b.id}>{backendLabel(b.backend)} · {formatBytes(b.size, getLocale())}{b.recommended ? ` (${t('engines.recommended')})` : ''}</option>
                  {/each}
                </select>
              </label>
            {/if}
            <button type="button" class="btn btn-primary" disabled={!inTauri || e.builds.length === 0} onclick={() => act(() => ipc.installEngine(e.id, chosenBuild(e)))}>
              <Download size={16} aria-hidden="true" />{e.installed ? t('engines.update') : t('engines.install')}
            </button>
          {/if}
          {#if e.installed}
            {#if e.capabilities.includes('chat')}
              <button type="button" class="btn" onclick={() => act(() => ipc.restartEngine(e.id))} disabled={e.state === 'starting'}>
                <RotateCw size={16} aria-hidden="true" />{t('engines.restart')}
              </button>
            {/if}
            {#if e.kind === 'python'}
              {#if confirmRepair === e.id}
                <button type="button" class="btn" onclick={() => act(async () => { await ipc.repairEngine(e.id); confirmRepair = null; })}>{t('engines.confirmRepair')}</button>
                <button type="button" class="btn" onclick={() => (confirmRepair = null)}>{t('common.cancel')}</button>
              {:else}
                <button type="button" class="btn" title={t('engines.repairHint')} onclick={() => (confirmRepair = e.id)}>
                  <Wrench size={16} aria-hidden="true" />{t('engines.repair')}
                </button>
              {/if}
            {/if}
            {#if confirmUninstall === e.id}
              <button type="button" class="btn danger" onclick={() => act(async () => { await ipc.uninstallEngine(e.id); confirmUninstall = null; })}>{t('engines.confirmUninstall')}</button>
              <button type="button" class="btn" onclick={() => (confirmUninstall = null)}>{t('common.cancel')}</button>
            {:else}
              <button type="button" class="btn" onclick={() => (confirmUninstall = e.id)}><Trash2 size={16} aria-hidden="true" />{t('engines.uninstall')}</button>
            {/if}
          {/if}
          <button type="button" class="btn" onclick={() => toggleLogs(e)} aria-expanded={logs[e.id] != null} disabled={!inTauri}>
            <ScrollText size={16} aria-hidden="true" />{t('engines.logs')}
          </button>
        </div>
        {#if !e.installed && e.builds.length === 0}
          <p class="note note-info">{t('engines.unavailable')}</p>
        {/if}
        {#if !e.installed && e.builds[0]}
          <p class="muted small">{t('engines.installHint', { size: formatBytes(e.builds.find((b) => b.id === chosenBuild(e))?.size ?? 0, getLocale()) })}</p>
        {/if}
      {/if}

      {#if logs[e.id] != null}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex (scrollable region must be reachable by keyboard) -->
        <pre class="log" tabindex="0" aria-label={t('engines.logs')}>{logs[e.id] || t('settings.logging.previewEmpty')}</pre>
      {/if}
    </article>
  {/each}

  {#if UPCOMING.length}
  <h2 class="later-title">{t('engines.upcoming')}</h2>
  <div class="later">
    {#each UPCOMING as u (u.id)}
      <article class="card muted-card">
        <header>
          <Cpu size={18} aria-hidden="true" />
          <div class="title">
            <h3>{u.name}</h3>
            <span class="muted small">{t(`engines.capabilities.${u.caps}`)}</span>
          </div>
          <span class="state">{t('views.milestone', { milestone: u.milestone })}</span>
        </header>
      </article>
    {/each}
  </div>
  {/if}
</div>

<style>
  .view {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 900px;
    height: 100%;
    margin: 0 auto;
    padding: var(--space-6) var(--space-4) var(--space-10);
    overflow-y: auto;
  }

  h1 {
    font-size: var(--text-2xl);
  }

  .muted {
    color: var(--text-muted);
  }

  .small {
    font-size: var(--text-s);
  }

  .card {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4) var(--space-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-l);
    background: var(--surface);
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .title {
    display: flex;
    flex: 1;
    flex-direction: column;
  }

  h2 {
    font-size: var(--text-l);
  }

  h3 {
    margin: 0;
    font-size: var(--text-m);
  }

  .state {
    padding: 2px 10px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text-muted);
    font-size: var(--text-xs);
    white-space: nowrap;
  }

  .state-running {
    background: var(--ok-bg);
    color: var(--ok);
  }

  .state-starting,
  .state-installing {
    background: var(--busy-bg);
    color: var(--busy);
  }

  .state-crashed {
    background: var(--error-bg);
    color: var(--error);
  }

  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-6);
    margin: 0;
    font-size: var(--text-s);
  }

  .facts dt {
    color: var(--text-muted);
  }

  .facts dd {
    margin: 0;
  }

  .actions,
  .progress {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }

  .progress :global([role='progressbar']) {
    flex: 1;
    min-width: 160px;
  }

  .btn.small {
    min-height: 30px;
    font-size: var(--text-s);
  }

  .btn.danger {
    border-color: var(--error);
    color: var(--error);
  }

  .log {
    max-height: 280px;
    margin: 0;
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    background: var(--bg);
    font-family: var(--font-mono);
    font-size: 11.5px;
    white-space: pre-wrap;
    overflow: auto;
  }

  .later-title {
    margin-top: var(--space-4);
    font-size: var(--text-m);
    color: var(--text-muted);
  }

  .later {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: var(--space-2);
  }

  .muted-card {
    padding: var(--space-3) var(--space-4);
    opacity: 0.75;
  }
</style>
