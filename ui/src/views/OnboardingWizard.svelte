<script lang="ts">
  import { onMount } from 'svelte';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import Cpu from '@lucide/svelte/icons/cpu';
  import MemoryStick from '@lucide/svelte/icons/memory-stick';
  import MonitorCog from '@lucide/svelte/icons/monitor-cog';
  import HardDrive from '@lucide/svelte/icons/hard-drive';
  import FolderSearch from '@lucide/svelte/icons/folder-search';
  import Zap from '@lucide/svelte/icons/zap';
  import Gauge from '@lucide/svelte/icons/gauge';
  import Turtle from '@lucide/svelte/icons/turtle';
  import Snail from '@lucide/svelte/icons/snail';
  import CircleX from '@lucide/svelte/icons/circle-x';
  import { getLocale, LOCALES, t, type Locale } from '../lib/i18n/index.svelte';
  import { inTauri, ipc, type StarterPick, type StorageOptions } from '../lib/ipc';
  import { settings } from '../lib/settings.svelte';
  import { downloads, hardware, models } from '../lib/stores.svelte';
  import { router } from '../lib/router.svelte';
  import { formatBytes, formatMb } from '../lib/format';
  import { capabilityFits, offeredCapabilities, type CapLevel } from '../lib/modelFilters';
  import { describeError } from '../lib/feedback';
  import SpeedBadge from '../lib/components/SpeedBadge.svelte';

  const STEPS = ['welcome', 'hardware', 'folder', 'pack', 'start'] as const;
  const LEVEL_ICONS = { fast: Zap, usable: Gauge, slow: Turtle, too_slow: Snail, needs_gpu: CircleX, wont_run: CircleX };

  let step = $state(0);
  let heading: HTMLElement | undefined = $state();
  let error = $state('');
  let busy = $state(false);

  let storage = $state<StorageOptions | null>(null);
  let chosenDir = $state('');
  let picks = $state<StarterPick[]>([]);
  let selected = $state<Record<string, boolean>>({});

  let profile = $derived(hardware.view?.profile ?? null);
  let budget = $derived(hardware.view?.budget ?? null);
  let caps = $derived(capabilityFits(models.list));
  let offered = $derived(offeredCapabilities(models.list));
  /** Chat is at best slow: say so and explain that smaller models are suggested. */
  let weak = $derived(caps.chat === 'slow' || caps.chat === 'too_slow' || caps.chat === 'wont_run');
  let key = (p: StarterPick) => `${p.model_id}/${p.variant_id}`;
  let chosenPicks = $derived(picks.filter((p) => selected[key(p)] && !p.installed));
  let totalBytes = $derived(chosenPicks.reduce((s, p) => s + p.size, 0));
  let freeBytes = $derived.by(() => {
    const disks = storage?.disks ?? [];
    const match = disks
      .filter((d) => chosenDir.toLowerCase().startsWith(d.mount_point.toLowerCase()))
      .sort((a, b) => b.mount_point.length - a.mount_point.length)[0];
    return match?.available_bytes ?? null;
  });
  let notEnough = $derived(freeBytes !== null && totalBytes * 1.1 > freeBytes);

  $effect(() => {
    void step;
    heading?.focus();
  });

  onMount(() => {
    if (!hardware.view?.profile && !hardware.probing) void hardware.load();
  });

  async function enter(next: number) {
    error = '';
    if (next === 2 && !storage) {
      try {
        storage = await ipc.getStorageOptions();
        if (!chosenDir) chosenDir = storage.current ?? bestDisk(storage) ?? storage.suggested;
      } catch (e) {
        error = describeError(e);
      }
    }
    step = next;
  }

  function bestDisk(s: StorageOptions): string | undefined {
    const fixed = s.disks.filter((d) => !d.removable);
    const ssd = fixed.filter((d) => d.kind === 'ssd' && d.available_bytes > 50 * 1024 ** 3);
    const pool = ssd.length ? ssd : fixed;
    return [...pool].sort((a, b) => b.available_bytes - a.available_bytes)[0]?.suggested_dir;
  }

  async function next() {
    error = '';
    busy = true;
    try {
      if (STEPS[step] === 'folder') {
        settings.apply(await ipc.setDataDir(chosenDir));
        picks = await ipc.getStarterPack();
        selected = Object.fromEntries(picks.map((p) => [key(p), !p.installed]));
      }
      await enter(step + 1);
    } catch (e) {
      error = describeError(e);
    } finally {
      busy = false;
    }
  }

  async function pickFolder() {
    const dir = await openDialog({ directory: true, multiple: false, title: t('wizard.folder.pickTitle') });
    if (typeof dir === 'string') chosenDir = dir;
  }

  async function finish(startDownloads: boolean) {
    error = '';
    busy = true;
    try {
      if (startDownloads) {
        for (const p of chosenPicks) await ipc.downloadVariant(p.model_id, p.variant_id);
        // Plug and play: the chat engine downloads alongside the chat model.
        if (chosenPicks.some((p) => p.modality === 'llm')) await ipc.ensureEngines().catch(() => undefined);
      }
      await settings.update({ onboarded: true });
      void downloads.load();
      void models.load();
      router.go(startDownloads && chosenPicks.length ? 'models' : 'workshop');
    } catch (e) {
      error = t('wizard.start.error', { error: describeError(e) });
    } finally {
      busy = false;
    }
  }

  async function chooseLanguage(locale: Locale) {
    await settings.update({ language: locale }).catch((e) => (error = describeError(e)));
  }

  function capLevel(level: CapLevel | null): CapLevel {
    return level ?? 'wont_run';
  }
</script>

<div class="overlay">
  <div class="wizard" role="dialog" aria-modal="true" aria-labelledby="wizard-heading">
    <ol class="steps" aria-label={t('wizard.title')}>
      {#each STEPS as s, i (s)}
        <li class:done={i < step} class:current={i === step} aria-current={i === step ? 'step' : undefined}>
          <span class="dot">{i + 1}</span><span class="label">{t(`wizard.steps.${s}`)}</span>
        </li>
      {/each}
    </ol>
    <p class="sr-only" aria-live="polite">{t('wizard.stepOf', { step: step + 1, total: STEPS.length })}</p>

    <div class="content">
      {#if STEPS[step] === 'welcome'}
        <h1 id="wizard-heading" tabindex="-1" bind:this={heading}>{t('wizard.welcome.title')}</h1>
        <p class="lead">{t('wizard.welcome.body')}</p>
        <fieldset class="langs">
          <legend>{t('wizard.welcome.language')}</legend>
          {#each LOCALES as locale (locale)}
            <label class="card" class:checked={settings.value.language === locale}>
              <input type="radio" name="wizard-lang" checked={settings.value.language === locale} onchange={() => chooseLanguage(locale)} />
              {t(`settings.languages.${locale}`)}
            </label>
          {/each}
        </fieldset>
      {:else if STEPS[step] === 'hardware'}
        <h1 id="wizard-heading" tabindex="-1" bind:this={heading}>{t('wizard.hardware.title')}</h1>
        {#if hardware.probing || !profile}
          <div class="probing" role="status">
            <span class="spinner" aria-hidden="true"></span>
            {hardware.error ? t('errors.generic', { error: hardware.error }) : t('wizard.hardware.probing')}
          </div>
        {:else}
          <ul class="hw">
            <li>
              <MonitorCog size={18} aria-hidden="true" />
              <span>
                <strong>{t('wizard.hardware.gpu')}</strong>
                {#if profile.gpus.length}
                  {#each profile.gpus as g (g.name)}
                    <span class="muted">{g.name} · {g.unified ? t('wizard.hardware.unified', { ram: formatMb(g.vram_mb, getLocale()) }) : formatMb(g.vram_mb, getLocale())}</span>
                    {#if settings.value.expert && g.compute_units}
                      <span class="muted small">
                        {t('wizard.hardware.gpuDetails', {
                          units: g.compute_units,
                          tflops: (g.fp32_tflops ?? 0).toLocaleString(getLocale(), { maximumFractionDigits: 1 }),
                          bandwidth: Math.round(g.memory_bandwidth_gbs ?? 0),
                        })}
                      </span>
                    {/if}
                  {/each}
                {:else}
                  <span class="muted">{t('wizard.hardware.noGpu')}</span>
                {/if}
              </span>
            </li>
            <li>
              <MemoryStick size={18} aria-hidden="true" />
              <span><strong>{t('wizard.hardware.ram')}</strong><span class="muted">{formatMb(profile.ram_mb, getLocale())}</span></span>
            </li>
            <li>
              <Cpu size={18} aria-hidden="true" />
              <span>
                <strong>{t('wizard.hardware.cpu')}</strong>
                <span class="muted">{profile.cpu.brand} · {t('wizard.hardware.cores', { count: profile.cpu.logical_cores })}</span>
                {#if settings.value.expert && profile.cpu.performance_cores && profile.cpu.efficiency_cores}
                  <span class="muted small">
                    {t('wizard.hardware.cpuHybrid', { performance: profile.cpu.performance_cores, efficiency: profile.cpu.efficiency_cores })}
                  </span>
                {/if}
              </span>
            </li>
          </ul>
          {#if budget}<p class="muted">{t('wizard.hardware.budget', { budget: formatMb(budget.main_mb, getLocale()), kind: t(`fit.budgetKind.${budget.kind}`) })}</p>{/if}
          <h2>{t('wizard.hardware.summary')}</h2>
          <p class="muted small">{t('wizard.hardware.speedNote')}</p>
          <ul class="caps">
            {#each offered as cap (cap)}
              {@const level = capLevel(caps[cap])}
              {@const Icon = LEVEL_ICONS[level]}
              <li class="cap lvl-{level}">
                <Icon size={16} aria-hidden="true" />
                <span>{t(`wizard.cap.${cap}`)}</span>
                <span class="lvl">{t(`wizard.capLevel.${level}`)}</span>
              </li>
            {/each}
          </ul>
          {#if weak}<p class="note note-warn">{t('wizard.hardware.weak')}</p>{/if}
          <button type="button" class="btn" onclick={() => hardware.load(true)}>{t('wizard.hardware.retry')}</button>
        {/if}
      {:else if STEPS[step] === 'folder'}
        <h1 id="wizard-heading" tabindex="-1" bind:this={heading}>{t('wizard.folder.title')}</h1>
        <p class="lead">{t('wizard.folder.body')}</p>
        {#if storage}
          <fieldset class="disks">
            <legend class="sr-only">{t('wizard.folder.title')}</legend>
            {#each storage.disks as d (d.mount_point)}
              <label class="card disk" class:checked={chosenDir === d.suggested_dir}>
                <input type="radio" name="wizard-disk" checked={chosenDir === d.suggested_dir} onchange={() => (chosenDir = d.suggested_dir)} />
                <HardDrive size={18} aria-hidden="true" />
                <span class="disk-text">
                  <strong>{d.mount_point} {d.name}</strong>
                  <span class="muted">
                    {t('wizard.folder.free', { free: formatBytes(d.available_bytes, getLocale()), total: formatBytes(d.total_bytes, getLocale()) })}
                    {#if d.kind !== 'unknown'} · {t(`wizard.folder.${d.kind}`)}{/if}
                  </span>
                  <span class="bar" aria-hidden="true"><span style:width="{Math.round((1 - d.available_bytes / d.total_bytes) * 100)}%"></span></span>
                </span>
              </label>
            {/each}
          </fieldset>
          <div class="chosen">
            <span class="muted">{t('wizard.folder.chosen')}:</span>
            <code>{chosenDir}</code>
            {#if inTauri}
              <button type="button" class="btn" onclick={pickFolder}><FolderSearch size={16} aria-hidden="true" />{t('wizard.folder.custom')}</button>
            {/if}
          </div>
        {/if}
      {:else if STEPS[step] === 'pack'}
        <h1 id="wizard-heading" tabindex="-1" bind:this={heading}>{t('wizard.pack.title')}</h1>
        <p class="lead">{t('wizard.pack.body')}</p>
        {#if picks.length === 0}
          <p class="note note-info">{t('wizard.pack.none')}</p>
        {:else}
          <ul class="picks">
            {#each picks as p (key(p))}
              <li>
                <label class="card pick" class:checked={selected[key(p)]}>
                  <input type="checkbox" bind:checked={selected[key(p)]} disabled={p.installed} />
                  <span class="pick-text">
                    <span class="muted small">{t(`modality.${p.modality}`)}</span>
                    <strong>{p.model_name} <span class="muted">{p.variant_label}</span></strong>
                  </span>
                  <SpeedBadge speed={p.speed} fit={p.fit} kind={budget?.kind ?? 'gpu'} showValue={settings.value.expert} />
                  <span class="size">{p.installed ? t('wizard.pack.installed') : formatBytes(p.size, getLocale())}</span>
                </label>
              </li>
            {/each}
          </ul>
          <p class="total">
            <strong>{t('wizard.pack.total', { size: formatBytes(totalBytes, getLocale()) })}</strong>
            {#if freeBytes !== null}<span class="muted"> · {t('wizard.pack.free', { free: formatBytes(freeBytes, getLocale()) })}</span>{/if}
          </p>
          {#if notEnough}<p class="note note-warn">{t('wizard.pack.notEnough')}</p>{/if}
        {/if}
      {:else}
        <h1 id="wizard-heading" tabindex="-1" bind:this={heading}>{t('wizard.start.title')}</h1>
        <p class="lead">{chosenPicks.length ? t('wizard.start.body', { count: chosenPicks.length, size: formatBytes(totalBytes, getLocale()) }) : t('wizard.start.nothing')}</p>
      {/if}

      {#if error}<p class="note note-error" role="alert">{error}</p>{/if}
    </div>

    <div class="nav">
      {#if step < STEPS.length - 1}
        <button type="button" class="skip" onclick={() => finish(false)} disabled={busy}>{t('wizard.skipWizard')}</button>
      {:else}
        <span></span>
      {/if}
      <div class="nav-buttons">
        {#if step > 0}
          <button type="button" class="btn" onclick={() => enter(step - 1)} disabled={busy}>{t('wizard.back')}</button>
        {/if}
        {#if STEPS[step] === 'start'}
          {#if chosenPicks.length}
            <button type="button" class="btn" onclick={() => finish(false)} disabled={busy}>{t('wizard.start.skip')}</button>
          {/if}
          <button type="button" class="btn btn-primary" onclick={() => finish(true)} disabled={busy}>
            {chosenPicks.length ? t('wizard.start.go') : t('wizard.start.finish')}
          </button>
        {:else}
          <button
            type="button"
            class="btn btn-primary"
            onclick={next}
            disabled={busy || (STEPS[step] === 'hardware' && !profile) || (STEPS[step] === 'folder' && !chosenDir) || (STEPS[step] === 'pack' && notEnough)}
          >
            {t('wizard.next')}
          </button>
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    background:
      radial-gradient(900px 480px at 80% 0%, var(--glow), transparent 65%),
      var(--bg);
    z-index: 60;
    overflow-y: auto;
  }

  .wizard {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
    width: min(760px, 100%);
    min-height: min(620px, 100%);
    padding: var(--space-6);
    border: 1px solid var(--border);
    border-radius: var(--radius-l);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .steps {
    display: flex;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .steps li {
    display: flex;
    flex: 1;
    align-items: center;
    gap: var(--space-2);
    padding-top: var(--space-2);
    border-top: 3px solid var(--surface-3);
    color: var(--text-muted);
    font-size: var(--text-xs);
  }

  .steps li.done {
    border-color: var(--ok);
  }

  .steps li.current {
    border-color: var(--accent);
    color: var(--text);
    font-weight: 600;
  }

  .dot {
    display: inline-grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: var(--surface-2);
    font-size: 11px;
  }

  .content {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: var(--space-4);
  }

  .content > .btn,
  .content > .note {
    align-self: flex-start;
  }

  h1 {
    font-size: var(--text-2xl);
  }

  h1:focus {
    outline: none;
  }

  h2 {
    font-size: var(--text-m);
  }

  .lead {
    max-width: 62ch;
    color: var(--text-muted);
    font-size: var(--text-l);
  }

  .muted {
    color: var(--text-muted);
  }

  .small {
    font-size: var(--text-xs);
  }

  fieldset {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    border: 0;
  }

  legend {
    margin-bottom: var(--space-2);
    font-weight: 600;
  }

  .langs {
    grid-template-columns: repeat(auto-fit, minmax(160px, 220px));
  }

  .card {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    cursor: pointer;
  }

  .card.checked {
    border-color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
  }

  .card:has(input:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }

  .card input[type='radio'] {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }

  .probing {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-6) 0;
    color: var(--text-muted);
  }

  .spinner {
    width: 22px;
    height: 22px;
    border: 3px solid var(--surface-3);
    border-top-color: var(--busy);
    border-radius: 50%;
    animation: spin 0.9s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .hw,
  .caps,
  .picks {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .hw li {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
  }

  .hw li span {
    display: flex;
    flex-direction: column;
  }

  .caps {
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  }

  .cap {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-m);
  }

  .cap .lvl {
    margin-left: auto;
    font-size: var(--text-s);
  }

  .lvl-fast {
    background: var(--ok-bg);
    color: var(--ok);
  }

  .lvl-usable {
    background: var(--speed-usable-bg);
    color: var(--speed-usable);
  }

  .lvl-slow {
    background: var(--speed-slow-bg);
    color: var(--speed-slow);
  }

  .lvl-too_slow,
  .lvl-needs_gpu,
  .lvl-wont_run {
    background: var(--error-bg);
    color: var(--error);
  }

  .disk-text,
  .pick-text {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .bar {
    height: 4px;
    margin-top: 4px;
    border-radius: 2px;
    background: var(--surface-3);
    overflow: hidden;
  }

  .bar span {
    display: block;
    height: 100%;
    background: var(--text-faint);
  }

  .chosen {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }

  code {
    padding: 2px 6px;
    border-radius: var(--radius-s);
    background: var(--bg);
    font-family: var(--font-mono);
    font-size: var(--text-s);
    word-break: break-all;
  }

  .size {
    min-width: 72px;
    color: var(--text-muted);
    font-size: var(--text-s);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .total {
    font-variant-numeric: tabular-nums;
  }

  .nav {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .nav-buttons {
    display: flex;
    gap: var(--space-2);
  }

  .skip {
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-muted);
    font-size: var(--text-s);
    text-decoration: underline;
  }

  @media (max-width: 560px) {
    .wizard {
      padding: var(--space-4);
    }

    .steps .label {
      display: none;
    }
  }
</style>
