<script lang="ts">
  import { onMount } from 'svelte';
  import { save } from '@tauri-apps/plugin-dialog';
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import Download from '@lucide/svelte/icons/download';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import { getLocale, LOCALES, t, type Locale } from '../lib/i18n/index.svelte';
  import { errorMessage, inTauri, ipc, type AppInfo, type AppSettings, type LogMode, type Theme } from '../lib/ipc';
  import { settings } from '../lib/settings.svelte';
  import { diagnosticsFileName, formatBytes } from '../lib/format';
  import { logToApp } from '../lib/errors';
  import { openExternal } from '../lib/external';
  import LibrarySettings from './settings/LibrarySettings.svelte';
  import { updates } from '../lib/updates.svelte';
  import NetworkSettings from './settings/NetworkSettings.svelte';
  import ReportProblem from './settings/ReportProblem.svelte';
  import SlotHost from '../lib/components/SlotHost.svelte';

  const THEMES: Theme[] = ['system', 'light', 'dark'];
  const LOG_MODES: LogMode[] = ['normal', 'verbose'];
  const TAIL_LINES = 200;
  const SITE = 'https://aitransformation.pl';
  const LICENSE_URL = 'https://opensource.org/license/mit';

  let info = $state<AppInfo | null>(null);
  let tail = $state('');
  let saveError = $state('');
  let exportStatus = $state<{ kind: 'busy' | 'ok' | 'error'; text: string } | null>(null);

  async function change(patch: Partial<AppSettings>) {
    saveError = '';
    try {
      await settings.update(patch);
      if (patch.log_mode) await refreshTail();
    } catch (e) {
      saveError = t('settings.saveFailed', { error: errorMessage(e) });
      logToApp('error', `settings update failed: ${errorMessage(e)}`);
    }
  }

  async function refreshTail() {
    if (!inTauri) return;
    try {
      tail = await ipc.getLogTail(TAIL_LINES);
    } catch (e) {
      tail = errorMessage(e);
    }
  }

  async function openFolder() {
    try {
      await ipc.openLogDir();
    } catch (e) {
      logToApp('warn', `open log dir failed: ${errorMessage(e)}`);
    }
  }

  async function exportBundle() {
    const dest = await save({
      defaultPath: diagnosticsFileName(),
      filters: [{ name: 'ZIP', extensions: ['zip'] }],
    });
    if (!dest) return;
    exportStatus = { kind: 'busy', text: t('settings.logging.exporting') };
    try {
      const summary = await ipc.exportDiagnostics(dest);
      exportStatus = {
        kind: 'ok',
        text: t('settings.logging.exported', { path: summary.path, size: formatBytes(summary.bytes, getLocale()) }),
      };
    } catch (e) {
      exportStatus = { kind: 'error', text: t('settings.logging.exportFailed', { error: errorMessage(e) }) };
    }
    await refreshTail();
  }

  onMount(async () => {
    if (!inTauri) return;
    try {
      info = await ipc.getAppInfo();
    } catch (e) {
      logToApp('warn', `get_app_info failed: ${errorMessage(e)}`);
    }
    await refreshTail();
  });
</script>

<div class="settings">
  <h1>{t('settings.title')}</h1>

  {#if saveError}<p class="note note-error" role="alert">{saveError}</p>{/if}

  <section aria-labelledby="s-general">
    <h2 id="s-general">{t('settings.general')}</h2>

    <div class="field">
      <label for="language">{t('settings.language')}</label>
      <select
        id="language"
        value={settings.value.language}
        onchange={(e) => change({ language: e.currentTarget.value as Locale })}
      >
        {#each LOCALES as locale (locale)}
          <option value={locale}>{t(`settings.languages.${locale}`)}</option>
        {/each}
      </select>
    </div>

    <fieldset class="field">
      <legend>{t('settings.theme')}</legend>
      <div class="segmented">
        {#each THEMES as theme (theme)}
          <label class:checked={settings.value.theme === theme}>
            <input
              type="radio"
              name="theme"
              value={theme}
              checked={settings.value.theme === theme}
              onchange={() => change({ theme })}
            />
            {t(`settings.themes.${theme}`)}
          </label>
        {/each}
      </div>
    </fieldset>
  </section>

  <LibrarySettings {change} />

  <NetworkSettings />

  {#if settings.value.expert}
  <section aria-labelledby="s-advanced">
    <h2 id="s-advanced">{t('settings.advanced')}</h2>
    <h3>{t('settings.logging.title')}</h3>
    <p class="muted">{t('settings.logging.description')}</p>

    <fieldset class="field">
      <legend>{t('settings.logging.mode')}</legend>
      <div class="options">
        {#each LOG_MODES as mode (mode)}
          <label class="option" class:checked={settings.value.log_mode === mode}>
            <input
              type="radio"
              name="log_mode"
              value={mode}
              checked={settings.value.log_mode === mode}
              onchange={() => change({ log_mode: mode })}
            />
            <span>
              <strong>{t(`settings.logging.${mode}`)}</strong>
              <span class="muted">{t(`settings.logging.${mode}Hint`)}</span>
            </span>
          </label>
        {/each}
      </div>
    </fieldset>

    {#if settings.logModeOverridden}
      <p class="note note-info">{t('settings.logging.overridden')}</p>
    {/if}

    <label class="check" class:disabled={settings.value.log_mode !== 'verbose'}>
      <input
        type="checkbox"
        checked={settings.value.log_prompts}
        disabled={settings.value.log_mode !== 'verbose'}
        onchange={(e) => change({ log_prompts: e.currentTarget.checked })}
        aria-describedby="log-prompts-hint"
      />
      <span>
        {t('settings.logging.prompts')}
        <span id="log-prompts-hint" class="muted">{t('settings.logging.promptsHint')}</span>
      </span>
    </label>
    {#if settings.value.log_prompts}
      <p class="note note-warn" role="status">{t('settings.logging.promptsHint')}</p>
    {/if}

    {#if inTauri}
      <div class="actions">
        <button type="button" class="btn" onclick={openFolder}>
          <FolderOpen size={16} aria-hidden="true" />
          {t('settings.logging.openFolder')}
        </button>
        <button type="button" class="btn" onclick={exportBundle} disabled={exportStatus?.kind === 'busy'}>
          <Download size={16} aria-hidden="true" />
          {t('settings.logging.export')}
        </button>
      </div>
      <p class="muted small">{t('settings.logging.exportHint')}</p>
      {#if exportStatus}
        <p
          class="note"
          class:note-info={exportStatus.kind === 'busy'}
          class:note-ok={exportStatus.kind === 'ok'}
          class:note-error={exportStatus.kind === 'error'}
          role="status"
        >
          {exportStatus.text}
        </p>
      {/if}

      {#if info}
        <p class="muted small">{t('settings.logging.location')}: <code>{info.log_dir}</code></p>
      {/if}

      <div class="tail-head">
        <h4>{t('settings.logging.preview')}</h4>
        <button type="button" class="btn" onclick={refreshTail}>
          <RefreshCw size={14} aria-hidden="true" />
          {t('settings.logging.refresh')}
        </button>
      </div>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (scrollable region must be reachable by keyboard) -->
      <pre class="tail" tabindex="0" aria-label={t('settings.logging.preview')}>{tail || t('settings.logging.previewEmpty')}</pre>
    {:else}
      <p class="note note-info">{t('settings.desktopOnly')}</p>
    {/if}
  </section>

  {:else}
  <p class="note note-info">{t('settings.basicNote')}</p>
  {/if}

  <ReportProblem {info} />

  <section aria-labelledby="s-about">
    <h2 id="s-about">{t('settings.about')}</h2>
    <p class="muted">
      {t('app.name')} · {t('settings.version')}
      {info?.version ?? '0.2.1'}{#if info} · {info.os}/{info.arch} · CPU × {info.cpu_logical_cores}{/if}
    </p>
    <div class="updates">
      <label class="check">
        <input type="checkbox" checked={settings.value.update_check} onchange={(e) => change({ update_check: e.currentTarget.checked })} />
        {t('updates.auto')}
      </label>
      <button type="button" class="btn" disabled={!inTauri || updates.checking || updates.installing} onclick={() => updates.check()}>
        <RefreshCw size={16} aria-hidden="true" />{updates.checking ? t('updates.checking') : t('updates.checkNow')}
      </button>
      {#if updates.available}
        <span role="status">{t('updates.available', { version: updates.available.version })}</span>
        <button type="button" class="btn btn-primary" disabled={updates.installing} onclick={() => updates.install()}>
          {updates.available.manual ? t('updates.download') : t('updates.install')}
        </button>
      {:else if updates.lastCheck === 'none'}
        <span class="muted" role="status">{t('updates.upToDate')}</span>
      {/if}
      {#if updates.error}<span class="note note-error small" role="alert">{updates.error}</span>{/if}
    </div>
    <!-- What the edition adds about itself (Pro: its licence). -->
    <SlotHost name="about" />
    <dl class="about">
      <div><dt>{t('settings.aboutInfo.author')}</dt><dd>Łukasz Poźniak</dd></div>
      <div><dt>{t('settings.aboutInfo.company')}</dt><dd>AI Transformation Łukasz Poźniak</dd></div>
      <div>
        <dt>{t('settings.aboutInfo.website')}</dt>
        <dd><a href={SITE} target="_blank" rel="noopener" onclick={(e) => openExternal(e, SITE)}>aitransformation.pl</a></dd>
      </div>
      <div>
        <dt>{t('settings.aboutInfo.license')}</dt>
        <dd>
          <a href={LICENSE_URL} target="_blank" rel="noopener" onclick={(e) => openExternal(e, LICENSE_URL)}>MIT</a>
          <span class="muted small">© 2026 Łukasz Poźniak. {t('settings.aboutInfo.licenseNote')}</span>
        </dd>
      </div>
    </dl>
  </section>
</div>

<style>
  .updates {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
    align-items: center;
  }

  .updates .check {
    display: inline-flex;
    gap: var(--space-2);
    align-items: center;
  }

  .settings {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
    max-width: 760px;
    height: 100%;
    margin: 0 auto;
    padding: var(--space-6) var(--space-4) var(--space-10);
    overflow-y: auto;
  }

  h1 {
    font-size: var(--text-2xl);
  }

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

  h3 {
    font-size: var(--text-m);
  }

  h4 {
    margin: 0;
    font-size: var(--text-s);
  }

  .muted {
    color: var(--text-muted);
  }

  .small {
    font-size: var(--text-s);
  }

  .about {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    font-size: var(--text-s);
  }

  .about > div {
    display: grid;
    grid-template-columns: 110px 1fr;
    gap: var(--space-3);
  }

  .about dt {
    color: var(--text-muted);
  }

  .about dd {
    display: flex;
    flex-direction: column;
    margin: 0;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    border: 0;
  }

  legend,
  .field > label {
    margin-bottom: var(--space-1);
    font-weight: 600;
    font-size: var(--text-s);
  }

  select {
    max-width: 280px;
  }

  .segmented {
    display: inline-flex;
    flex-wrap: wrap;
    width: fit-content;
    padding: 3px;
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    background: var(--bg);
  }

  .segmented label {
    padding: var(--space-1) var(--space-3);
    border-radius: var(--radius-s);
    cursor: pointer;
  }

  .segmented label.checked {
    background: var(--accent);
    color: var(--accent-contrast);
  }

  .segmented input,
  .option input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }

  .segmented label:has(input:focus-visible),
  .option:has(input:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }

  .options {
    display: grid;
    gap: var(--space-2);
  }

  .option {
    position: relative;
    display: flex;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    cursor: pointer;
  }

  .option.checked {
    border-color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
  }

  .option span,
  .check span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .option .muted,
  .check .muted {
    font-size: var(--text-s);
  }

  .check {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
  }

  .check input {
    margin-top: 4px;
  }

  .check.disabled {
    opacity: 0.6;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  code {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    word-break: break-all;
  }

  .tail-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .tail-head .btn {
    min-height: 30px;
    font-size: var(--text-s);
  }

  .tail {
    max-height: 320px;
    margin: 0;
    padding: var(--space-3);
    border-radius: var(--radius-m);
    background: var(--bg);
    border: 1px solid var(--border);
    font-family: var(--font-mono);
    font-size: 11.5px;
    line-height: 1.45;
    white-space: pre-wrap;
    word-break: break-word;
    overflow: auto;
  }
</style>
