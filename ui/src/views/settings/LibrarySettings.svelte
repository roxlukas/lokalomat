<script lang="ts">
  import { onMount } from 'svelte';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import FolderSearch from '@lucide/svelte/icons/folder-search';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import { getLocale, t } from '../../lib/i18n/index.svelte';
  import { inTauri, ipc, type AppSettings } from '../../lib/ipc';
  import { settings } from '../../lib/settings.svelte';
  import { hardware, models } from '../../lib/stores.svelte';
  import { formatMb } from '../../lib/format';
  import { describeError } from '../../lib/feedback';

  let { change }: { change: (patch: Partial<AppSettings>) => Promise<void> } = $props();

  /** Fixed context windows offered in Expert mode (tokens). */
  const CONTEXT_SIZES = [4096, 8192, 16384, 32768, 65536, 131072];

  let status = $state<{ text: string; kind: 'ok' | 'error' } | null>(null);
  let tokenSet = $state(false);
  let token = $state('');
  let limitMb = $state(Math.max(1, Math.round((settings.value.bandwidth_limit_kbps ?? 10240) / 1024)));
  let unified = $state(settings.value.unified_memory_percent);

  let profile = $derived(hardware.view?.profile ?? null);
  let budget = $derived(hardware.view?.budget ?? null);

  onMount(async () => {
    if (!inTauri) return;
    tokenSet = await ipc.getHfTokenStatus().catch(() => false);
  });

  async function report(fn: () => Promise<unknown>, ok?: string) {
    status = null;
    try {
      await fn();
      if (ok) status = { text: ok, kind: 'ok' };
    } catch (e) {
      status = { text: describeError(e), kind: 'error' };
    }
  }

  async function changeFolder() {
    const dir = await openDialog({ directory: true, multiple: false, title: t('wizard.folder.pickTitle') });
    if (typeof dir !== 'string') return;
    await report(async () => {
      settings.apply(await ipc.setDataDir(dir));
    }, t('settings.data.changed'));
  }

  async function saveToken(e: SubmitEvent) {
    e.preventDefault();
    await report(async () => {
      tokenSet = await ipc.setHfToken(token);
      token = '';
    }, t('settings.downloads.tokenSaved'));
  }

  async function removeToken() {
    await report(async () => {
      tokenSet = await ipc.setHfToken('');
    }, t('settings.downloads.tokenRemoved'));
  }

  async function setLimit(enabled: boolean) {
    await change({ bandwidth_limit_kbps: enabled ? Math.max(1, limitMb) * 1024 : null });
  }

  async function saveUnified() {
    await change({ unified_memory_percent: unified });
    await hardware.load();
    await models.load();
  }

  async function rerunWizard() {
    await change({ onboarded: false });
  }
</script>

<section aria-labelledby="s-data">
  <h2 id="s-data">{t('settings.data.title')}</h2>
  <p class="muted">{t('settings.data.hint')}</p>
  <p><code>{settings.value.data_dir ?? t('settings.data.none')}</code></p>
  {#if inTauri}
    <div class="actions">
      <button type="button" class="btn" onclick={changeFolder}><FolderSearch size={16} aria-hidden="true" />{t('settings.data.change')}</button>
      {#if settings.value.data_dir}
        <button type="button" class="btn" onclick={() => report(() => ipc.openDataDir())}><FolderOpen size={16} aria-hidden="true" />{t('settings.data.open')}</button>
      {/if}
      <button type="button" class="btn" onclick={rerunWizard}>{t('settings.data.rerunWizard')}</button>
    </div>
    <p class="muted small">{t('settings.data.note')}</p>
  {/if}
</section>

{#if settings.value.expert}
<section aria-labelledby="s-hw">
  <div class="head">
    <h2 id="s-hw">{t('settings.hardware.title')}</h2>
    <button type="button" class="btn" onclick={() => hardware.load(true)} disabled={hardware.probing}>
      <RefreshCw size={14} aria-hidden="true" />{hardware.probing ? t('settings.hardware.probing') : t('settings.hardware.refresh')}
    </button>
  </div>
  {#if profile}
    <dl class="facts">
      <div>
        <dt>{t('wizard.hardware.gpu')}</dt>
        <dd>
          {#each profile.gpus as g (g.name)}
            <span>{g.name} · {formatMb(g.vram_mb, getLocale())}{#if g.driver} · {t('settings.hardware.driver', { version: g.driver })}{/if}</span>
          {:else}
            {t('wizard.hardware.noGpu')}
          {/each}
        </dd>
      </div>
      <div><dt>{t('wizard.hardware.ram')}</dt><dd>{formatMb(profile.ram_mb, getLocale())}</dd></div>
      <div><dt>{t('wizard.hardware.cpu')}</dt><dd>{profile.cpu.brand} · {t('wizard.hardware.cores', { count: profile.cpu.logical_cores })}</dd></div>
      <div><dt>{t('settings.hardware.features')}</dt><dd>{profile.cpu.features.join(', ') || '—'}</dd></div>
      <div><dt>{t('settings.hardware.backends')}</dt><dd>{profile.backends.map((b) => b.toUpperCase()).join(', ')}</dd></div>
      {#if budget}
        <div>
          <dt>{t('settings.hardware.budget')}</dt>
          <dd>
            {formatMb(budget.main_mb, getLocale())} ({t(`fit.budgetKind.${budget.kind}`)})
            {#if budget.offload_mb}<span class="muted"> {t('settings.hardware.offload', { offload: formatMb(budget.offload_mb, getLocale()) })}</span>{/if}
          </dd>
        </div>
      {/if}
    </dl>
  {:else}
    <p class="muted">{hardware.probing ? t('wizard.hardware.probing') : t('models.noHardware')}</p>
  {/if}
  <div class="field">
    <label for="unified">{t('settings.hardware.unified', { percent: unified })}</label>
    <input id="unified" type="range" min="30" max="90" step="5" bind:value={unified} onchange={saveUnified} aria-describedby="unified-hint" />
    <span id="unified-hint" class="muted small">{t('settings.hardware.unifiedHint')}</span>
  </div>
  <div class="field">
    <label for="ctx">{t('settings.hardware.context')}</label>
    <select id="ctx" value={settings.value.context_size === null ? 'auto' : String(settings.value.context_size)}
      onchange={(e) => change({ context_size: e.currentTarget.value === 'auto' ? null : Number(e.currentTarget.value) })}
      aria-describedby="ctx-hint">
      <option value="auto">{t('settings.hardware.contextAuto')}</option>
      {#each CONTEXT_SIZES as size (size)}
        <option value={String(size)}>{t('settings.hardware.contextTokens', { count: size / 1024 })}</option>
      {/each}
    </select>
    <span id="ctx-hint" class="muted small">{t('settings.hardware.contextHint')}</span>
  </div>
  <label class="check">
    <input type="checkbox" checked={settings.value.keep_llm_loaded} onchange={(e) => change({ keep_llm_loaded: e.currentTarget.checked })} />
    <span>{t('settings.hardware.keepLlm')}<span class="muted small">{t('settings.hardware.keepLlmHint')}</span></span>
  </label>
</section>

<section aria-labelledby="s-dl">
  <h2 id="s-dl">{t('settings.downloads.title')}</h2>
  {#if inTauri}
    <form class="field" onsubmit={saveToken}>
      <label for="hf-token"><KeyRound size={14} aria-hidden="true" /> {t('settings.downloads.token')}</label>
      <span class="muted small">{t('settings.downloads.tokenHint')}</span>
      <span class="state" class:ok={tokenSet}>{tokenSet ? t('settings.downloads.tokenSet') : t('settings.downloads.tokenNone')}</span>
      <div class="inline">
        <input id="hf-token" type="password" autocomplete="off" spellcheck="false" placeholder="hf_…" bind:value={token} />
        <button type="submit" class="btn" disabled={!token.trim()}>{t('settings.downloads.tokenSave')}</button>
        {#if tokenSet}<button type="button" class="btn" onclick={removeToken}>{t('settings.downloads.tokenRemove')}</button>{/if}
      </div>
    </form>
  {/if}
  <label class="check">
    <input type="checkbox" checked={settings.value.bandwidth_limit_kbps !== null} onchange={(e) => setLimit(e.currentTarget.checked)} />
    <span>{t('settings.downloads.limit')}<span class="muted small">{t('settings.downloads.limitHint')}</span></span>
  </label>
  {#if settings.value.bandwidth_limit_kbps !== null}
    <div class="inline">
      <label for="limit">{t('settings.downloads.limitValue')}</label>
      <input id="limit" type="number" min="1" max="1000" bind:value={limitMb} onchange={() => setLimit(true)} />
    </div>
  {/if}
  {#if status}
    <p class="note" class:note-ok={status.kind === 'ok'} class:note-error={status.kind === 'error'} role="status">{status.text}</p>
  {/if}
</section>
{/if}

<style>
  section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-l);
    background: var(--surface);
  }

  h2 {
    font-size: var(--text-l);
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .muted {
    color: var(--text-muted);
  }

  .small {
    font-size: var(--text-s);
  }

  code {
    font-family: var(--font-mono);
    font-size: var(--text-s);
    word-break: break-all;
  }

  .actions,
  .inline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }

  .facts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: var(--space-3);
    margin: 0;
    font-size: var(--text-s);
  }

  .facts dt {
    color: var(--text-muted);
  }

  .facts dd {
    display: flex;
    flex-direction: column;
    margin: 0;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .field > label {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-weight: 600;
    font-size: var(--text-s);
  }

  input[type='range'] {
    max-width: 320px;
  }

  select {
    max-width: 280px;
  }

  input[type='password'] {
    width: min(320px, 100%);
    font-family: var(--font-mono);
  }

  input[type='number'] {
    width: 90px;
  }

  .state {
    font-size: var(--text-s);
    color: var(--text-muted);
  }

  .state.ok {
    color: var(--ok);
  }

  .check {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
  }

  .check input {
    margin-top: 4px;
  }

  .check span {
    display: flex;
    flex-direction: column;
  }
</style>
