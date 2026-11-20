<script lang="ts">
  import ScanLine from '@lucide/svelte/icons/scan-line';
  import Smartphone from '@lucide/svelte/icons/smartphone';
  import { t } from '../lib/i18n/index.svelte';
  import { api, describe, inApp, type MobileStatus } from './api';

  /** Pairing (V9): scan the QR code from Settings › Network on the computer. */
  let { onPaired }: { onPaired: (s: MobileStatus) => void } = $props();

  let link = $state('');
  let deviceName = $state(t('mobile.pair.defaultName'));
  let code = $state<string | null>(null);
  let computer = $state('');
  let error = $state('');
  let busy = $state(false);

  // Suggest the phone's own name and model; keep the generic default otherwise.
  const fallbackName = t('mobile.pair.defaultName');
  api
    .deviceName()
    .then((name) => {
      if (name && deviceName === fallbackName) deviceName = name;
    })
    .catch(() => {});

  async function scan() {
    error = '';
    try {
      const scanner = await import('@tauri-apps/plugin-barcode-scanner');
      const perm = await scanner.requestPermissions();
      if (perm !== 'granted') {
        error = t('mobile.pair.noCamera');
        return;
      }
      const result = await scanner.scan({ windowed: false, formats: [scanner.Format.QRCode] });
      link = result.content;
      await start();
    } catch {
      error = t('mobile.pair.scanFailed');
    }
  }

  async function start() {
    if (!link.trim()) return;
    error = '';
    busy = true;
    try {
      const started = await api.pairBegin(link.trim(), deviceName);
      code = started.code;
      computer = started.computer_name;
      onPaired(await api.pairWait());
    } catch (e) {
      error = describe(e);
      code = null;
    } finally {
      busy = false;
    }
  }

  async function cancel() {
    await api.pairCancel().catch(() => undefined);
    code = null;
    busy = false;
  }
</script>

<section class="pair">
  <Smartphone size={40} aria-hidden="true" />
  <h1>{t('mobile.pair.title')}</h1>

  {#if code}
    <p>{t('mobile.pair.compare', { computer: computer || t('mobile.pair.computer') })}</p>
    <p class="code" aria-label={t('mobile.pair.codeLabel')}>{code}</p>
    <p class="muted">{t('mobile.pair.confirmOnComputer')}</p>
    <button type="button" class="btn" onclick={cancel}>{t('common.cancel')}</button>
  {:else}
    <p class="muted">{t('mobile.pair.body')}</p>
    <label class="field">
      <span class="small muted">{t('mobile.pair.name')}</span>
      <input type="text" bind:value={deviceName} maxlength="60" />
    </label>
    <button type="button" class="btn btn-primary big" onclick={scan} disabled={busy || !inApp}>
      <ScanLine size={20} aria-hidden="true" />{t('mobile.pair.scan')}
    </button>
    <details>
      <summary class="small">{t('mobile.pair.manual')}</summary>
      <label class="field">
        <span class="small muted">{t('mobile.pair.link')}</span>
        <textarea rows="3" bind:value={link} placeholder="lokalomat://pair?…"></textarea>
      </label>
      <button type="button" class="btn" onclick={start} disabled={busy || !link.trim()}>{t('mobile.pair.connect')}</button>
    </details>
  {/if}
  {#if error}<p class="note note-error" role="alert">{error}</p>{/if}
</section>

<style>
  .pair {
    display: grid;
    gap: var(--space-3);
    justify-items: center;
    max-width: 420px;
    margin: 0 auto;
    padding: var(--space-8) var(--space-4);
    text-align: center;
  }

  h1 {
    font-size: var(--text-xl);
  }

  .muted {
    color: var(--text-muted);
  }

  .small {
    font-size: var(--text-s);
  }

  .field {
    display: grid;
    gap: var(--space-1);
    width: 100%;
    text-align: left;
  }

  .big {
    min-height: 48px;
    padding: 0 var(--space-6);
    font-size: var(--text-l);
  }

  details {
    display: grid;
    gap: var(--space-2);
    width: 100%;
    text-align: left;
  }

  textarea {
    width: 100%;
  }

  .code {
    font-family: var(--font-mono);
    font-size: 44px;
    font-weight: 700;
    letter-spacing: 0.12em;
  }
</style>
