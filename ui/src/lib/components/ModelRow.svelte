<script lang="ts">
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import Download from '@lucide/svelte/icons/download';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import Star from '@lucide/svelte/icons/star';
  import Check from '@lucide/svelte/icons/check';
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { getLocale, t } from '../i18n/index.svelte';
  import { inTauri, ipc, type BudgetKind, type ModelView, type VariantView } from '../ipc';
  import { formatBytes, formatMb, formatPercent } from '../format';
  import { primaryVariant, scoreFor, type SortKey } from '../modelFilters';
  import { downloads, models } from '../stores.svelte';
  import { settings } from '../settings.svelte';
  import { describeEnqueue, describeError } from '../feedback';
  import FitBadge from './FitBadge.svelte';
  import SpeedBadge from './SpeedBadge.svelte';
  import LicenseBadge from './LicenseBadge.svelte';
  import ProgressBar from './ProgressBar.svelte';

  let {
    model,
    budgetKind = 'gpu',
    hasDataDir,
    sortKey = 'default',
    onMessage,
  }: {
    model: ModelView;
    budgetKind?: BudgetKind;
    hasDataDir: boolean;
    sortKey?: SortKey;
    onMessage: (text: string, kind: 'ok' | 'error' | 'info') => void;
  } = $props();

  let open = $state(false);
  let confirming = $state<string | null>(null);
  let busy = $state(false);
  let primary = $derived(primaryVariant(model));
  let description = $derived(getLocale() === 'en' ? model.description_en : model.description_pl);
  let conditions = $derived(getLocale() === 'en' ? model.license.conditions_en : model.license.conditions_pl);
  let score = $derived(scoreFor(model, sortKey));
  let scoreLabel = $derived(
    sortKey === 'speed'
      ? `${formatNumber(score ?? 0)} tok/s`
      : `${sortKey === 'overall' ? t('models.sort.overallShort') : sortKey.slice(4)} ${formatNumber(score ?? 0)}/5`,
  );
  let topCategories = $derived(
    model.scores
      ? Object.entries(model.scores.categories)
          .filter(([, c]) => c.n >= 3)
          .sort((a, b) => b[1].avg - a[1].avg)
          .slice(0, 4)
      : [],
  );

  function formatNumber(n: number): string {
    return n.toLocaleString(getLocale(), { maximumFractionDigits: 1, minimumFractionDigits: 1 });
  }
  let panelId = $derived(`model-${model.id.replace(/[^a-z0-9-]/gi, '_')}`);

  /** Bytes for a variant including live progress events. */
  function liveDownloaded(v: VariantView): number {
    const items = downloads.items.filter((i) => i.model_id === model.id && i.variant_id === v.id && i.status !== 'completed');
    const stale = items.reduce((s, i) => s + i.downloaded, 0);
    const live = items.reduce((s, i) => s + downloads.downloaded(i), 0);
    return v.downloaded - stale + live;
  }

  async function run(action: () => Promise<void>) {
    busy = true;
    try {
      await action();
    } catch (e) {
      onMessage(describeError(e), 'error');
    } finally {
      busy = false;
      confirming = null;
      void models.load();
      void downloads.load();
    }
  }

  const download = (v: VariantView) =>
    run(async () => onMessage(describeEnqueue(await ipc.downloadVariant(model.id, v.id)), 'info'));

  const remove = (v: VariantView) =>
    run(async () => {
      if (model.custom) await ipc.removeCustomModel(model.id);
      else await ipc.deleteVariant(model.id, v.id);
      onMessage(t('models.removed', { name: `${model.name} ${v.label}` }), 'ok');
    });

  const makeDefault = (v: VariantView) => run(() => ipc.setDefaultModel(model.id, v.id));

  async function openLicense() {
    const url = model.license.url;
    if (!url) return;
    if (inTauri) await openUrl(url).catch((e) => onMessage(describeError(e), 'error'));
    else window.open(url, '_blank', 'noopener');
  }
</script>

{#snippet actions(v: VariantView)}
  {#if v.state === 'installed'}
    {#if model.default_variant === v.id}
      <span class="chip chip-ok"><Star size={12} aria-hidden="true" />{t('models.row.isDefault')}</span>
    {:else}
      <button type="button" class="btn small" disabled={busy} onclick={() => makeDefault(v)}>
        <Star size={14} aria-hidden="true" />{t('models.row.setDefault')}
      </button>
    {/if}
    {#if confirming === v.id}
      <button type="button" class="btn small danger" disabled={busy} onclick={() => remove(v)}>
        {t('models.row.confirmDelete')}
      </button>
      <button type="button" class="btn small" onclick={() => (confirming = null)}>{t('common.cancel')}</button>
    {:else}
      <button
        type="button"
        class="icon-btn"
        aria-label={t('models.row.deleteVariant', { variant: `${model.name} ${v.label}` })}
        title={t('models.row.delete')}
        onclick={() => (confirming = v.id)}
      >
        <Trash2 size={16} aria-hidden="true" />
      </button>
    {/if}
  {:else if v.state === 'partial'}
    <span class="chip chip-warn">{t('models.row.partial')}</span>
    <button
      type="button"
      class="btn small accent"
      disabled={busy || !hasDataDir || !v.supported_here}
      onclick={() => download(v)}
    >
      <Download size={14} aria-hidden="true" />{t('models.row.downloadMissing')}
    </button>
  {:else if v.state === 'not_installed'}
    <button
      type="button"
      class="btn small"
      class:accent={v.id === primary?.id}
      disabled={busy || !hasDataDir || !v.supported_here}
      title={!hasDataDir ? t('errors.no_data_dir') : !v.supported_here ? t('models.row.notSupported') : undefined}
      onclick={() => download(v)}
    >
      <Download size={14} aria-hidden="true" />{t('models.row.download')}
    </button>
  {:else}
    {@const fraction = v.size ? liveDownloaded(v) / v.size : null}
    <span class="progress">
      <span class="state state-{v.state}">
        {v.state === 'downloading' && fraction !== null
          ? t('models.row.downloading', { percent: formatPercent(fraction, getLocale()) })
          : t(`models.row.${v.state}`)}
      </span>
      <ProgressBar
        value={fraction}
        label={t('models.row.progressLabel', { name: `${model.name} ${v.label}` })}
        tone={v.state === 'failed' ? 'error' : v.state === 'paused' ? 'muted' : 'busy'}
      />
    </span>
  {/if}
{/snippet}

<article class="row" class:open>
  <div class="summary">
    <button type="button" class="toggle" aria-expanded={open} aria-controls={panelId} onclick={() => (open = !open)}>
      <ChevronDown size={16} aria-hidden="true" class="chev" />
      <span class="title">
        <span class="name">{model.name}</span>
        <span class="meta">{model.publisher || t('models.custom.source.disk')} · {t(`modality.${model.modality}`)}</span>
      </span>
      {#if sortKey !== 'default'}
        {#if score !== null}
          <span class="score" title={t('models.bench.tooltip', { name: model.scores?.name ?? '', quant: model.scores?.quantization ?? '?' })}>{scoreLabel}</span>
        {:else if model.modality === 'llm'}
          <span class="score none">{t('models.bench.noScore')}</span>
        {/if}
      {/if}
    </button>
    <span class="cell fit">
      <SpeedBadge speed={primary?.speed} fit={primary?.fit ?? null} kind={budgetKind} showValue={settings.value.expert} />
    </span>
    <span class="cell size">{primary ? formatBytes(primary.size, getLocale()) : ''}</span>
    <span class="cell lic"><LicenseBadge license={model.license} /></span>
    <span class="cell act">
      {#if primary}{@render actions(primary)}{/if}
    </span>
  </div>

  {#if open}
    <div class="details" id={panelId}>
      {#if description}<p class="desc">{description}</p>{/if}
      {#if model.custom}<p class="muted small">{t('models.custom.license')}</p>{/if}

      <dl class="facts">
        {#if model.languages.length}
          <div><dt>{t('models.details.languages')}</dt><dd>{model.languages.join(', ')}</dd></div>
        {/if}
        {#if settings.value.expert}
          <div>
            <dt>{t('models.details.engine')}</dt>
            <dd>{t('models.details.engineState', { engine: t(`engines.${model.engine}`) })}</dd>
          </div>
        {/if}
        {#if model.scores}
          <div>
            <dt>{t('models.bench.title')}</dt>
            <dd>
              {t('models.bench.summary', {
                overall: formatNumber(model.scores.overall),
                quant: model.scores.quantization ?? '?',
                speed: model.scores.tokens_per_second ? formatNumber(model.scores.tokens_per_second) : '—',
              })}
              {#if topCategories.length}
                <span class="muted small"> · {topCategories.map(([c, s]) => `${c} ${formatNumber(s.avg)}`).join(' · ')}</span>
              {/if}
            </dd>
          </div>
        {/if}
        {#if model.license.name}
          <div>
            <dt>{t('models.details.license')}</dt>
            <dd>
              {model.license.name}{#if conditions} – {conditions}{/if}
              {#if model.license.url}
                <button type="button" class="link" onclick={openLicense}>
                  {t('license.open')}<ExternalLink size={12} aria-hidden="true" />
                </button>
              {/if}
              {#if model.license.gated}<span class="muted small"> · {t('license.gated')}</span>{/if}
            </dd>
          </div>
        {/if}
      </dl>

      {#if settings.value.expert}
      <table class="variants">
        <caption class="sr-only">{t('models.details.variants', { name: model.name })}</caption>
        <thead>
          <tr>
            <th scope="col">{t('models.details.variant')}</th>
            <th scope="col">{t('models.details.size')}</th>
            <th scope="col">{t('models.details.memory')}</th>
            <th scope="col">{t('models.details.fit')}</th>
            <th scope="col">{t('models.details.speed')}</th>
            <th scope="col"><span class="sr-only">{t('models.details.actions')}</span></th>
          </tr>
        </thead>
        <tbody>
          {#each model.variants as v (v.id)}
            <tr class:unsupported={!v.supported_here}>
              <th scope="row">
                {v.label}
                {#if v.recommended}<span class="muted small">({t('models.row.recommended')})</span>{/if}
                {#if v.state === 'installed'}<Check size={14} aria-label={t('models.row.installed')} class="ok" />{/if}
                {#if v.files > 1}<span class="muted small">· {t('models.details.files', { count: v.files })}</span>{/if}
              </th>
              <td>{formatBytes(v.size, getLocale())}</td>
              <td>{formatMb(v.fit?.estimate_mb ?? v.memory_estimate_mb, getLocale())}</td>
              <td><FitBadge report={v.fit} kind={budgetKind} /></td>
              <td><SpeedBadge speed={v.speed} fit={v.fit} kind={budgetKind} showValue /></td>
              <td class="act">
                {#if v.supported_here}{@render actions(v)}{:else}<span class="muted small">{t('models.row.notSupported')}</span>{/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      {/if}
    </div>
  {/if}
</article>

<style>
  .row {
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    background: var(--surface);
  }

  .row.open {
    border-color: var(--border-strong);
  }

  .summary {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto 84px auto minmax(150px, auto);
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3) var(--space-2) 0;
  }

  .toggle {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    padding: var(--space-2) var(--space-3);
    border: 0;
    background: none;
    text-align: left;
  }

  .toggle :global(.chev) {
    flex: none;
    color: var(--text-muted);
    transition: transform var(--dur-fast) var(--ease);
  }

  .open .toggle :global(.chev) {
    transform: rotate(180deg);
  }

  .title {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .name {
    font-weight: 600;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .meta,
  .muted {
    color: var(--text-muted);
  }

  .meta,
  .small {
    font-size: var(--text-s);
  }

  .score {
    flex: none;
    margin-left: auto;
    padding: 1px 8px;
    border-radius: var(--radius-s);
    background: var(--info-bg);
    color: var(--info);
    font-size: var(--text-xs);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .score.none {
    background: var(--surface-2);
    color: var(--text-muted);
    font-weight: 400;
  }

  .toggle {
    width: 100%;
  }

  .size {
    color: var(--text-muted);
    font-size: var(--text-s);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .act {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-2);
  }

  .btn.small {
    min-height: 30px;
    padding: 0 var(--space-3);
    font-size: var(--text-s);
  }

  .btn.danger {
    border-color: var(--error);
    color: var(--error);
  }

  .btn.accent {
    border-color: var(--accent);
    color: var(--accent);
    font-weight: 600;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    border-radius: 999px;
    font-size: var(--text-xs);
  }

  .chip-ok {
    background: var(--ok-bg);
    color: var(--ok);
  }

  .chip-warn {
    background: var(--warn-bg);
    color: var(--warn);
  }

  .progress {
    display: grid;
    gap: 4px;
    min-width: 150px;
  }

  .state {
    font-size: var(--text-xs);
    color: var(--busy);
  }

  .state-failed {
    color: var(--error);
  }

  .state-paused {
    color: var(--text-muted);
  }

  .details {
    display: grid;
    gap: var(--space-3);
    padding: 0 var(--space-4) var(--space-4) calc(var(--space-3) + 24px);
  }

  .desc {
    max-width: 70ch;
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

  .link {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    margin-left: var(--space-2);
    padding: 0;
    border: 0;
    background: none;
    color: var(--focus);
    text-decoration: underline;
  }

  .variants {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-s);
  }

  .variants th,
  .variants td {
    padding: var(--space-2);
    border-top: 1px solid var(--border);
    text-align: left;
    font-variant-numeric: tabular-nums;
  }

  .variants thead th {
    border-top: 0;
    color: var(--text-muted);
    font-weight: 500;
  }

  .variants tbody th {
    font-weight: 500;
  }

  .variants :global(.ok) {
    color: var(--ok);
    vertical-align: -2px;
  }

  .unsupported {
    opacity: 0.6;
  }

  @media (max-width: 860px) {
    .summary {
      grid-template-columns: minmax(0, 1fr) auto;
    }

    .size,
    .lic {
      display: none;
    }

    .act {
      grid-column: 1 / -1;
      justify-content: flex-start;
      padding-left: var(--space-3);
    }

    .details {
      padding-left: var(--space-4);
      overflow-x: auto;
    }
  }
</style>
