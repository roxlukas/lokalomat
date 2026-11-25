<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import LinkSimple from 'phosphor-svelte/lib/LinkSimpleIcon';
  import PencilSimple from 'phosphor-svelte/lib/PencilSimpleIcon';
  import Scan from 'phosphor-svelte/lib/ScanIcon';
  import X from 'phosphor-svelte/lib/XIcon';
  import { t } from '../lib/i18n/index.svelte';
  import { api, describe, inApp, type MobileStatus } from './api';
  import Spinner from './Spinner.svelte';

  /**
   * Pairing (V9). The camera opens at once behind a transparent window: aim it at
   * the QR code from Settings › Network on the computer. "Paste link" and the
   * phone's name are quiet actions below. Then both screens show the same code.
   */
  let {
    onPaired,
    onClose,
    overlay = false,
  }: {
    onPaired: (s: MobileStatus) => void;
    /** Shown as a close button; leave out where the screen cannot be closed. */
    onClose?: () => void;
    /** Covers the whole app (a sheet) instead of sitting in the page. */
    overlay?: boolean;
  } = $props();

  type Mode = 'scan' | 'paste' | 'code';
  type Scanner = typeof import('@tauri-apps/plugin-barcode-scanner');

  const fallbackName = t('mobile.pair.defaultName');
  let mode = $state<Mode>(inApp ? 'scan' : 'paste');
  let link = $state('');
  let deviceName = $state(fallbackName);
  let editingName = $state(false);
  let code = $state<string | null>(null);
  let computer = $state('');
  let error = $state('');
  let busy = $state(false);
  let scanner: Scanner | null = null;
  /** Counts pairing attempts: an answer that arrives after a cancel or close belongs to an old one. */
  let attempt = 0;

  // Suggest the phone's own name and model; keep the generic default otherwise.
  api
    .deviceName()
    .then((name) => {
      if (name && deviceName === fallbackName) deviceName = name;
    })
    .catch(() => {});

  onMount(() => {
    if (mode === 'scan') void beginScan();
  });

  onDestroy(() => {
    attempt += 1;
    endScanUi();
    void scanner?.cancel().catch(() => undefined);
  });

  /** The page must be see-through while the camera is behind it. */
  function endScanUi() {
    document.documentElement.classList.remove('pair-scan');
  }

  async function beginScan() {
    error = '';
    mode = 'scan';
    document.documentElement.classList.add('pair-scan');
    try {
      scanner ??= await import('@tauri-apps/plugin-barcode-scanner');
      if ((await scanner.requestPermissions()) !== 'granted') throw new Error('camera');
      const result = await scanner.scan({ windowed: true, formats: [scanner.Format.QRCode] });
      endScanUi();
      link = result.content;
      await start();
    } catch (e) {
      endScanUi();
      // Stopped on purpose (paste link, close): nothing to report.
      if (mode !== 'scan') return;
      mode = 'paste';
      error = e instanceof Error && e.message === 'camera' ? t('mobile.pair.noCamera') : t('mobile.pair.scanFailed');
    }
  }

  async function toPaste() {
    mode = 'paste';
    error = '';
    await scanner?.cancel().catch(() => undefined);
  }

  async function close() {
    attempt += 1;
    void api.pairCancel().catch(() => undefined);
    mode = 'paste';
    await scanner?.cancel().catch(() => undefined);
    onClose?.();
  }

  async function start() {
    if (!link.trim()) return;
    error = '';
    busy = true;
    const mine = ++attempt;
    try {
      const started = await api.pairBegin(link.trim(), deviceName);
      if (mine !== attempt) return;
      code = started.code;
      computer = started.computer_name;
      mode = 'code';
      const paired = await api.pairWait();
      if (mine !== attempt) return;
      onPaired(paired);
    } catch (e) {
      // Cancelled meanwhile: the screen already moved on.
      if (mine !== attempt) return;
      error = describe(e);
      code = null;
      mode = 'paste';
    } finally {
      if (mine === attempt) busy = false;
    }
  }

  async function cancel() {
    attempt += 1;
    await api.pairCancel().catch(() => undefined);
    code = null;
    busy = false;
    if (inApp) void beginScan();
    else mode = 'paste';
  }

  function focusNow(node: HTMLElement) {
    node.focus();
  }

  function nameKey(e: KeyboardEvent) {
    if (e.key === 'Enter') editingName = false;
  }
</script>

<section class="screen" class:overlay class:scan={mode === 'scan'}>
  {#if mode === 'scan'}
    <header class="top">
      {#if onClose}
        <button type="button" class="round" aria-label={t('common.close')} title={t('common.close')} onclick={close}>
          <X size={22} aria-hidden="true" />
        </button>
      {/if}
      <p class="title on-camera">{t('mobile.pair.scanTitle')}</p>
    </header>
    <div class="finder" aria-hidden="true">
      <i class="corner tl"></i><i class="corner tr"></i><i class="corner bl"></i><i class="corner br"></i>
    </div>
    <footer class="bottom">
      <p class="hint on-camera">{t('mobile.pair.where')}</p>
      <div class="actions">
        <button type="button" class="pill on-camera" onclick={toPaste}>
          <LinkSimple size={18} aria-hidden="true" />{t('mobile.pair.pasteShort')}
        </button>
        {#if editingName}
          <input class="pill on-camera name" bind:value={deviceName} maxlength="60" aria-label={t('mobile.pair.name')} use:focusNow onblur={() => (editingName = false)} onkeydown={nameKey} />
        {:else}
          <button type="button" class="pill on-camera" onclick={() => (editingName = true)} aria-label={t('mobile.pair.name')}>
            <PencilSimple size={18} aria-hidden="true" />{deviceName}
          </button>
        {/if}
      </div>
    </footer>
  {:else if mode === 'paste'}
    <header class="top solid">
      {#if onClose}
        <button type="button" class="round" aria-label={t('common.close')} title={t('common.close')} onclick={close}>
          <X size={22} aria-hidden="true" />
        </button>
      {/if}
      <p class="title">{t('mobile.pair.pasteTitle')}</p>
    </header>
    <div class="body">
      {#if error}<p class="note note-error" role="alert">{error}</p>{/if}
      <textarea rows="4" bind:value={link} placeholder="lokalomat://pair?…" aria-label={t('mobile.pair.link')}></textarea>
      <button type="button" class="btn btn-primary big" onclick={start} disabled={busy || !link.trim()}>{t('mobile.pair.connect')}</button>
      {#if inApp}
        <button type="button" class="btn big" onclick={beginScan}>
          <Scan size={20} aria-hidden="true" />{t('mobile.pair.scan')}
        </button>
      {/if}
    </div>
  {:else}
    <div class="code-screen">
      <p class="muted">{computer || t('mobile.pair.computer')}</p>
      <p class="code" aria-label={t('mobile.pair.codeLabel')}>{code}</p>
      <p>{t('mobile.pair.codeQuestion')}</p>
      <p class="muted small waiting"><Spinner />{t('mobile.pair.waiting')}</p>
      <button type="button" class="btn big" onclick={cancel}>{t('common.cancel')}</button>
    </div>
  {/if}
</section>

<style>
  .screen {
    display: flex;
    flex-direction: column;
    min-height: 100%;
    padding: max(var(--space-8), env(safe-area-inset-top, 0px)) var(--space-4)
      max(var(--space-4), env(safe-area-inset-bottom, 0px));
  }

  .screen.overlay {
    position: fixed;
    inset: 0;
    z-index: 30;
    overflow: hidden;
    background: var(--bg);
  }

  /* The camera shows through. */
  .screen.scan {
    background: transparent;
  }

  .top {
    position: relative;
    z-index: 1;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 48px;
  }

  .title {
    margin: 0;
    font-size: var(--text-l);
    font-weight: 600;
  }

  .round {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border: 0;
    border-radius: 50%;
    background: color-mix(in srgb, #000 55%, transparent);
    color: #fff;
  }

  /* Text over the camera picture stays readable on any scene. */
  .on-camera {
    color: #fff;
    text-shadow: 0 1px 3px rgb(0 0 0 / 0.6);
  }

  .top .title.on-camera {
    padding: var(--space-1) var(--space-3);
    border-radius: var(--radius-pill);
    background: color-mix(in srgb, #000 45%, transparent);
    text-shadow: none;
  }

  .finder {
    position: relative;
    z-index: 0;
    align-self: center;
    width: min(70vw, 280px);
    aspect-ratio: 1;
    margin: auto 0;
    border-radius: 12px;
    /* dims everything outside the frame */
    box-shadow: 0 0 0 100vmax rgb(0 0 0 / 0.5);
  }

  .corner {
    position: absolute;
    width: 34px;
    height: 34px;
    border: 4px solid var(--accent);
  }

  .tl {
    top: -2px;
    left: -2px;
    border-right: 0;
    border-bottom: 0;
    border-top-left-radius: 14px;
  }

  .tr {
    top: -2px;
    right: -2px;
    border-left: 0;
    border-bottom: 0;
    border-top-right-radius: 14px;
  }

  .bl {
    bottom: -2px;
    left: -2px;
    border-right: 0;
    border-top: 0;
    border-bottom-left-radius: 14px;
  }

  .br {
    right: -2px;
    bottom: -2px;
    border-left: 0;
    border-top: 0;
    border-bottom-right-radius: 14px;
  }

  .bottom {
    position: relative;
    z-index: 1;
    display: grid;
    gap: var(--space-3);
    justify-items: center;
  }

  .hint {
    margin: 0;
    font-size: var(--text-s);
    text-align: center;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: var(--space-2);
  }

  .pill {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 48px;
    max-width: 60vw;
    padding: 0 var(--space-4);
    border: 1px solid rgb(255 255 255 / 0.35);
    border-radius: var(--radius-pill);
    background: color-mix(in srgb, #000 55%, transparent);
    font-size: var(--text-s);
    text-shadow: none;
  }

  .pill.name {
    width: 60vw;
  }

  .body {
    display: grid;
    align-content: start;
    gap: var(--space-3);
    padding-top: var(--space-4);
  }

  .body textarea {
    width: 100%;
  }

  .big {
    justify-content: center;
    width: 100%;
    min-height: 52px;
    font-size: var(--text-m, 1rem);
  }

  .code-screen {
    display: grid;
    flex: 1;
    align-content: center;
    justify-items: center;
    gap: var(--space-3);
    text-align: center;
  }

  .code-screen p {
    margin: 0;
  }

  .muted {
    color: var(--text-muted);
  }

  .small {
    font-size: var(--text-s);
  }

  .waiting {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .code {
    font-family: var(--font-mono);
    font-size: 44px;
    font-weight: 700;
    letter-spacing: 0.1em;
  }
</style>
