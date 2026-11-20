<script lang="ts">
  import Search from '@lucide/svelte/icons/search';
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import FilePlus from '@lucide/svelte/icons/file-plus';
  import CloudDownload from '@lucide/svelte/icons/cloud-download';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { t } from '../lib/i18n/index.svelte';
  import { EXPERT_MODALITIES, inTauri, ipc, MODALITIES } from '../lib/ipc';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import {
    activeFilterCount,
    DEFAULT_FILTERS,
    filterModels,
    isSortKey,
    parseHfReference,
    sortModels,
    type ModelFilters,
    type SortKey,
  } from '../lib/modelFilters';
  import { hardware, models } from '../lib/stores.svelte';
  import { settings } from '../lib/settings.svelte';
  import { router } from '../lib/router.svelte';
  import { describeEnqueue, describeError } from '../lib/feedback';
  import { readLocal, writeLocal } from '../lib/storage';
  import ModelRow from '../lib/components/ModelRow.svelte';

  type Tab = 'catalog' | 'custom';
  const TAB_KEY = 'lokalsi.models-tab';
  const SORT_KEY = 'lokalsi.models-sort';

  let storedTab = $state<Tab>(readLocal<Tab>(TAB_KEY, 'catalog'));
  let expert = $derived(settings.value.expert);
  // Custom GGUF models are an Expert feature (PRD §15).
  let tab = $derived<Tab>(expert ? storedTab : 'catalog');
  let filters = $state<ModelFilters>({ ...DEFAULT_FILTERS });
  let message = $state<{ text: string; kind: 'ok' | 'error' | 'info' } | null>(null);
  let hfRepo = $state('');
  let hfPath = $state('');
  let hfBusy = $state(false);

  let budgetKind = $derived(hardware.view?.budget?.kind ?? 'gpu');
  let hasDataDir = $derived(Boolean(settings.value.data_dir));
  // Basic mode hides Expert-only kinds of models (decision models, ADR 0024).
  let catalog = $derived(models.list.filter((m) => !m.custom && (expert || !EXPERT_MODALITIES.includes(m.modality))));
  // Only the kinds of models this edition's catalog has (the free edition: chat and decisions).
  let modalities = $derived(
    MODALITIES.filter((m) => (expert || !EXPERT_MODALITIES.includes(m)) && models.list.some((x) => x.modality === m)),
  );
  let custom = $derived(models.list.filter((m) => m.custom));
  let storedSort = readLocal<string>(SORT_KEY, 'default');
  let chosenSort = $state<SortKey>(isSortKey(storedSort) ? storedSort : 'default');
  let bench = $derived(models.bench);
  // Without benchmark data (site did not respond) benchmark sorting is ignored.
  let sortKey = $derived<SortKey>(bench && expert ? chosenSort : 'default');
  let visible = $derived(sortModels(filterModels(catalog, filters), sortKey));
  let scoredCount = $derived(visible.filter((m) => m.scores).length);

  function setSort(value: string) {
    chosenSort = isSortKey(value) ? value : 'default';
    writeLocal(SORT_KEY, chosenSort);
  }

  async function openBenchmark() {
    if (!bench) return;
    if (inTauri) await openUrl(bench.source_url).catch((e) => notify(describeError(e), 'error'));
    else window.open(bench.source_url, '_blank', 'noopener');
  }

  function setTab(next: Tab) {
    storedTab = next;
    writeLocal(TAB_KEY, next);
  }

  function onTabKey(e: KeyboardEvent) {
    if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
      e.preventDefault();
      const next: Tab = tab === 'catalog' ? 'custom' : 'catalog';
      setTab(next);
      document.getElementById(`tab-${next}`)?.focus();
    }
  }

  function notify(text: string, kind: 'ok' | 'error' | 'info') {
    message = { text, kind };
  }

  async function addFromDisk() {
    const path = await openDialog({
      multiple: false,
      directory: false,
      filters: [{ name: t('models.custom.fileFilter'), extensions: ['gguf'] }],
    });
    if (typeof path !== 'string') return;
    try {
      const view = await ipc.addCustomModelFile(path);
      notify(t('models.custom.added', { name: view.name }), 'ok');
      await models.load();
    } catch (e) {
      notify(describeError(e), 'error');
    }
  }

  function onRepoInput() {
    const parsed = parseHfReference(hfRepo);
    if (parsed) {
      hfRepo = parsed.repo;
      hfPath = parsed.path;
    }
  }

  async function addFromHf(e: SubmitEvent) {
    e.preventDefault();
    hfBusy = true;
    try {
      const outcome = await ipc.addCustomModelHf(hfRepo, hfPath);
      notify(describeEnqueue(outcome), 'info');
      hfRepo = '';
      hfPath = '';
      await models.load();
    } catch (err) {
      notify(describeError(err), 'error');
    } finally {
      hfBusy = false;
    }
  }
</script>

{#snippet chip(label: string, active: boolean, toggle: () => void)}
  <button type="button" class="chip" class:active aria-pressed={active} onclick={toggle}>{label}</button>
{/snippet}

<div class="view">
  <header class="top">
    <h1>{t('views.models.title')}</h1>
    {#if expert}
    <div class="tabs" role="tablist" aria-label={t('views.models.title')}>
      {#each ['catalog', 'custom'] as const as id (id)}
        <button
          type="button"
          role="tab"
          id="tab-{id}"
          aria-selected={tab === id}
          aria-controls="panel-{id}"
          tabindex={tab === id ? 0 : -1}
          class:selected={tab === id}
          onclick={() => setTab(id)}
          onkeydown={onTabKey}
        >
          {t(`models.tabs.${id}`)}{#if id === 'custom' && custom.length}<span class="count">{custom.length}</span>{/if}
        </button>
      {/each}
    </div>
    {/if}
  </header>

  {#if !hasDataDir}
    <div class="note note-warn banner">
      <span>{t('models.noDataDir')}</span>
      <button type="button" class="btn" onclick={() => router.go('settings')}>
        <FolderOpen size={16} aria-hidden="true" />{t('models.chooseFolder')}
      </button>
    </div>
  {/if}
  {#if !hardware.view?.budget}
    <p class="note note-info">{hardware.probing ? t('models.probing') : t('models.noHardware')}</p>
  {/if}
  <div class="live" aria-live="polite">
    {#if message}
      <p class="note" class:note-ok={message.kind === 'ok'} class:note-error={message.kind === 'error'} class:note-info={message.kind === 'info'}>
        {message.text}
      </p>
    {/if}
  </div>

  {#if tab === 'catalog'}
    <div id="panel-catalog" role="tabpanel" aria-labelledby="tab-catalog" class="panel">
      <div class="toolbar">
        <label class="search">
          <Search size={16} aria-hidden="true" />
          <span class="sr-only">{t('models.search')}</span>
          <input type="search" placeholder={t('models.search')} bind:value={filters.query} />
        </label>
        <div class="chips" role="group" aria-label={t('models.filters.modality')}>
          {@render chip(t('models.filters.all'), filters.modality === 'all', () => (filters.modality = 'all'))}
          {#each modalities as m (m)}
            {@render chip(t(`modality.${m}`), filters.modality === m, () => (filters.modality = filters.modality === m ? 'all' : m))}
          {/each}
        </div>
        <div class="chips" role="group" aria-label={t('models.filters.label')}>
          {@render chip(t('models.filters.fits'), filters.fitsHardware, () => (filters.fitsHardware = !filters.fitsHardware))}
          {#if expert}
            {@render chip(t('models.filters.commercial'), filters.commercial, () => (filters.commercial = !filters.commercial))}
            {@render chip(t('models.filters.eu'), filters.euAllowed, () => (filters.euAllowed = !filters.euAllowed))}
          {/if}
          {@render chip(t('models.filters.polish'), filters.polish, () => (filters.polish = !filters.polish))}
          {@render chip(t('models.filters.installed'), filters.installed === 'installed', () =>
            (filters.installed = filters.installed === 'installed' ? 'all' : 'installed'))}
          {@render chip(t('models.filters.notInstalled'), filters.installed === 'not_installed', () =>
            (filters.installed = filters.installed === 'not_installed' ? 'all' : 'not_installed'))}
          {#if activeFilterCount(filters)}
            <button type="button" class="reset" onclick={() => (filters = { ...DEFAULT_FILTERS })}>{t('models.filters.reset')}</button>
          {/if}
        </div>
      </div>

      <div class="listhead">
        <p class="muted small" aria-live="polite">{t('models.count', { count: visible.length, total: catalog.length })}</p>
        {#if bench && expert}
          <label class="sort">
            <span>{t('models.sort.label')}</span>
            <select value={sortKey} onchange={(e) => setSort(e.currentTarget.value)}>
              <option value="default">{t('models.sort.default')}</option>
              <option value="overall">{t('models.sort.overall')}</option>
              <option value="speed">{t('models.sort.speed')}</option>
              <optgroup label={t('models.sort.categories')}>
                {#each bench.categories as [name, prompts] (name)}
                  <option value="cat:{name}">{name} ({t('models.sort.prompts', { count: prompts })})</option>
                {/each}
              </optgroup>
            </select>
          </label>
        {/if}
      </div>
      {#if bench && sortKey !== 'default'}
        <p class="note note-info bench-note">
          {t('models.sort.note', { scored: scoredCount, date: bench.generated_at.slice(0, 10) })}
          <button type="button" class="reset" onclick={openBenchmark}>{t('models.sort.source', { license: bench.license })}</button>
        </p>
      {/if}

      {#if models.error}
        <p class="note note-error">{t('errors.generic', { error: models.error })}</p>
      {:else if !models.loaded}
        <div class="skeleton" aria-hidden="true"></div>
        <div class="skeleton" aria-hidden="true"></div>
      {:else if visible.length === 0}
        <p class="empty">{t('models.empty')}</p>
      {:else}
        <div class="list">
          {#each visible as model (model.id)}
            <ModelRow {model} {budgetKind} {hasDataDir} {sortKey} onMessage={notify} />
          {/each}
        </div>
      {/if}
    </div>
  {:else}
    <div id="panel-custom" role="tabpanel" aria-labelledby="tab-custom" class="panel">
      <p class="muted">{t('models.custom.description')}</p>
      <div class="add">
        <div class="card">
          <h2><FilePlus size={16} aria-hidden="true" />{t('models.custom.fromDiskTitle')}</h2>
          <p class="muted small">{t('models.custom.fromDiskHint')}</p>
          <button type="button" class="btn" onclick={addFromDisk} disabled={!inTauri}>{t('models.custom.fromDisk')}</button>
        </div>
        <form class="card" onsubmit={addFromHf}>
          <h2><CloudDownload size={16} aria-hidden="true" />{t('models.custom.fromHf')}</h2>
          <label>
            <span>{t('models.custom.repo')}</span>
            <input bind:value={hfRepo} oninput={onRepoInput} placeholder="org/model-GGUF" required autocomplete="off" spellcheck="false" />
          </label>
          <label>
            <span>{t('models.custom.path')}</span>
            <input bind:value={hfPath} placeholder="model-Q4_K_M.gguf" required autocomplete="off" spellcheck="false" />
          </label>
          <p class="muted small">{t('models.custom.pasteHint')}</p>
          <button type="submit" class="btn btn-primary" disabled={hfBusy || !hasDataDir || !hfRepo || !hfPath}>
            {hfBusy ? t('models.custom.adding') : t('models.custom.add')}
          </button>
        </form>
      </div>
      {#if custom.length === 0}
        <p class="empty">{t('models.custom.empty')}</p>
      {:else}
        <div class="list">
          {#each custom as model (model.id)}
            <ModelRow {model} {budgetKind} {hasDataDir} {sortKey} onMessage={notify} />
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .view {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    height: 100%;
    max-width: 1100px;
    margin: 0 auto;
    padding: var(--space-6) var(--space-4) var(--space-10);
    overflow-y: auto;
  }

  .top {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  h1 {
    font-size: var(--text-2xl);
  }

  .tabs {
    display: inline-flex;
    padding: 3px;
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    background: var(--bg);
  }

  .tabs button {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-3);
    border: 0;
    border-radius: var(--radius-s);
    background: none;
  }

  .tabs button.selected {
    background: var(--accent);
    color: var(--accent-contrast);
  }

  .count {
    min-width: 18px;
    padding: 0 5px;
    border-radius: 9px;
    background: var(--surface-3);
    color: var(--text);
    font-size: var(--text-xs);
  }

  .banner {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .live:empty {
    display: none;
  }

  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .toolbar {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .search {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    max-width: 420px;
    height: 36px;
    padding: 0 var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-pill);
    background: var(--surface);
    color: var(--text-muted);
  }

  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: none;
    outline: none;
    box-shadow: none;
    min-height: 0;
    padding: 0;
    color: var(--text);
  }

  .search:focus-within {
    outline: 2px solid var(--focus);
    outline-offset: 1px;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .chip {
    padding: 3px var(--space-3);
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    font-size: var(--text-s);
  }

  .chip:hover {
    border-color: var(--border-strong);
  }

  .chip.active {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-contrast);
  }

  .reset {
    border: 0;
    background: none;
    color: var(--focus);
    font-size: var(--text-s);
    text-decoration: underline;
  }

  .listhead {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .sort {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-s);
  }

  .sort select {
    max-width: 280px;
  }

  .bench-note .reset {
    margin-left: var(--space-1);
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .muted {
    color: var(--text-muted);
  }

  .small {
    font-size: var(--text-s);
  }

  .empty {
    padding: var(--space-8) 0;
    color: var(--text-muted);
    text-align: center;
  }

  .skeleton {
    height: 58px;
    border-radius: var(--radius-m);
    background: linear-gradient(90deg, var(--surface-2), var(--surface), var(--surface-2));
    background-size: 200% 100%;
    animation: shimmer 1.4s linear infinite;
  }

  @keyframes shimmer {
    to {
      background-position: -200% 0;
    }
  }

  .add {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
    gap: var(--space-3);
  }

  .card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    background: var(--surface);
  }

  .card h2 {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-m);
  }

  .card label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    font-size: var(--text-s);
  }

  .card input {
    font-family: var(--font-mono);
    font-size: var(--text-s);
  }
</style>
