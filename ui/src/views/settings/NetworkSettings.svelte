<script lang="ts">
  import Smartphone from '@lucide/svelte/icons/smartphone';
  import Unlink from '@lucide/svelte/icons/unlink';
  import Copy from '@lucide/svelte/icons/copy';
  import { getLocale, t } from '../../lib/i18n/index.svelte';
  import { EVENTS, inTauri, ipc, onEvent, type NetworkStatus, type PairingView } from '../../lib/ipc';
  import { describeError } from '../../lib/feedback';
  import { settings } from '../../lib/settings.svelte';
  import { qrPath } from '../../lib/qr';

  /** Settings › Sieć (V8): LAN sharing for paired phones (PRD §15 item 12). */
  let net = $state<NetworkStatus | null>(null);
  let pairing = $state<PairingView | null>(null);
  let outcome = $state<'paired' | 'rejected' | 'expired' | 'cancelled' | null>(null);
  let error = $state('');
  let busy = $state(false);
  let left = $state(0);
  let port = $state(settings.value.lan_port);
  let confirmUnpair = $state<string | null>(null);
  let copied = $state(false);

  async function refresh() {
    try {
      net = await ipc.getNetwork();
      if (net.pairing && net.pairing.state !== 'waiting' && net.pairing.state !== 'confirm') pairing = null;
      else pairing = net.pairing;
    } catch (e) {
      if (inTauri) error = describeError(e);
    }
  }

  $effect(() => {
    void refresh();
    const subs = [
      onEvent<PairingView>(EVENTS.pairingChanged, (view) => {
        if (view.state === 'waiting' || view.state === 'confirm') {
          pairing = view;
          outcome = null;
          left = view.expires_in;
        } else {
          pairing = null;
          outcome = view.state;
        }
      }),
      onEvent<null>(EVENTS.devicesChanged, () => void refresh()),
    ];
    return () => subs.forEach((s) => void s.then((off) => off()));
  });

  // Countdown of the QR code / the confirmation.
  $effect(() => {
    if (!pairing) return;
    left = pairing.expires_in;
    const timer = setInterval(() => {
      left = Math.max(0, left - 1);
      if (left === 0) void refresh();
    }, 1000);
    return () => clearInterval(timer);
  });

  async function act(fn: () => Promise<unknown>) {
    error = '';
    busy = true;
    try {
      await fn();
    } catch (e) {
      error = describeError(e);
    } finally {
      busy = false;
    }
  }

  async function toggle(enabled: boolean) {
    await act(async () => {
      net = await ipc.setLanSharing(enabled, settings.value.expert ? port : undefined);
      await settings.load();
      pairing = null;
      outcome = null;
    });
  }

  async function savePort() {
    if (!net?.enabled) return;
    await act(async () => {
      net = await ipc.setLanSharing(true, port);
      await settings.load();
    });
  }

  async function startPairing() {
    outcome = null;
    await act(async () => {
      pairing = await ipc.startPairing();
      left = pairing.expires_in;
    });
  }

  async function answer(match: boolean) {
    await act(() => ipc.confirmPairing(match));
  }

  async function unpair(id: string) {
    await act(async () => {
      net = await ipc.unpairDevice(id);
      confirmUnpair = null;
    });
  }

  async function copyLink() {
    if (!pairing) return;
    await navigator.clipboard.writeText(pairing.uri).catch(() => undefined);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  function when(secs: number | null): string {
    if (!secs) return t('network.never');
    return new Date(secs * 1000).toLocaleString(getLocale(), { dateStyle: 'medium', timeStyle: 'short' });
  }

  function mmss(s: number): string {
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  }
</script>

<section aria-labelledby="s-network">
  <h2 id="s-network">{t('network.title')}</h2>
  <p class="muted">{t('network.body')}</p>

  <label class="switch">
    <input type="checkbox" checked={net?.enabled ?? settings.value.lan_enabled} disabled={busy || !inTauri}
      onchange={(e) => toggle(e.currentTarget.checked)} />
    <span>{t('network.enable')}</span>
  </label>

  {#if settings.value.expert}
    <label class="inline">
      <span class="muted small">{t('network.port')}</span>
      <input type="number" min="1024" max="65535" bind:value={port} onchange={savePort} title={t('network.portTip')} />
    </label>
  {/if}

  {#if net?.error}
    <p class="note note-error" role="alert">{t(`network.errors.${net.error}`)}</p>
  {/if}

  {#if net?.running}
    <p class="muted small" title={t('network.addressTip')}>{t('network.running', { address: net.address ?? '' })}</p>

    {#if pairing?.state === 'confirm'}
      <div class="pairing confirm" role="group" aria-labelledby="pair-q">
        <p id="pair-q"><strong>{t('network.compare', { device: pairing.device_name ?? '' })}</strong></p>
        <p class="code" aria-label={t('network.codeLabel')}>{pairing.code}</p>
        <p class="muted small">{t('network.compareHint')}</p>
        <div class="actions">
          <button type="button" class="btn btn-primary" disabled={busy} onclick={() => answer(true)}>{t('network.match')}</button>
          <button type="button" class="btn danger" disabled={busy} onclick={() => answer(false)}>{t('network.noMatch')}</button>
        </div>
      </div>
    {:else if pairing?.state === 'waiting'}
      <div class="pairing" role="group" aria-label={t('network.qrLabel')}>
        {#if pairing.qr}
          {@const size = pairing.qr.size + 8}
          <svg class="qr" viewBox={`0 0 ${size} ${size}`} role="img" aria-label={t('network.qrLabel')} shape-rendering="crispEdges">
            <rect width={size} height={size} fill="var(--qr-light)" />
            <path d={qrPath(pairing.qr.size, pairing.qr.modules, 4)} fill="var(--qr-dark)" />
          </svg>
        {/if}
        <div class="pair-text">
          <p><strong>{t('network.scan')}</strong></p>
          <p class="muted small">{t('network.expires', { time: mmss(left) })}</p>
          <div class="actions">
            <button type="button" class="btn small" onclick={() => act(() => ipc.cancelPairing())}>{t('common.cancel')}</button>
            {#if settings.value.expert}
              <button type="button" class="btn small" onclick={copyLink} title={t('network.copyLinkTip')}>
                <Copy size={14} aria-hidden="true" />{copied ? t('jobs.copied') : t('network.copyLink')}
              </button>
            {/if}
          </div>
        </div>
      </div>
    {:else}
      <div class="actions">
        <button type="button" class="btn btn-primary" disabled={busy} onclick={startPairing}>
          <Smartphone size={16} aria-hidden="true" />{t('network.pair')}
        </button>
      </div>
    {/if}
  {/if}

  {#if outcome}
    <p class="note" class:note-info={outcome === 'paired'} class:note-warn={outcome !== 'paired'} role="status">
      {t(`network.outcome.${outcome}`)}
    </p>
  {/if}
  {#if error}<p class="note note-error" role="alert">{error}</p>{/if}

  <h3>{t('network.devices')}</h3>
  {#if !net || net.devices.length === 0}
    <p class="muted small">{t('network.noDevices')}</p>
  {:else}
    <ul class="devices">
      {#each net.devices as d (d.id)}
        <li>
          <Smartphone size={18} aria-hidden="true" />
          <div class="dev">
            <span class="dev-name">{d.name}</span>
            <span class="muted small">{t('network.pairedAt', { date: when(d.paired_at) })} · {t('network.lastSeen', { date: when(d.last_seen) })}</span>
          </div>
          {#if confirmUnpair === d.id}
            <button type="button" class="btn small btn-danger" disabled={busy} onclick={() => unpair(d.id)}>{t('network.unpairConfirm')}</button>
          {:else}
            <button type="button" class="btn small danger" onclick={() => (confirmUnpair = d.id)}>
              <Unlink size={14} aria-hidden="true" />{t('network.unpair')}
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

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

  h3 {
    font-size: var(--text-m);
  }

  .muted {
    color: var(--text-muted);
  }

  .small {
    font-size: var(--text-s);
  }

  .switch,
  .inline,
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }

  .inline input {
    width: 110px;
  }

  .pairing {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    align-items: center;
    padding: var(--space-4);
    border-radius: var(--radius-l);
    background: var(--surface-2);
  }

  .pairing.confirm {
    display: grid;
    gap: var(--space-2);
    justify-items: start;
  }

  .qr {
    width: 220px;
    height: 220px;
    border-radius: var(--radius-m);
  }

  .pair-text {
    display: grid;
    gap: var(--space-2);
    max-width: 320px;
  }

  .code {
    font-family: var(--font-mono);
    font-size: 40px;
    font-weight: 700;
    letter-spacing: 0.12em;
  }

  .devices {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .devices li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
  }

  .dev {
    display: grid;
    flex: 1;
    min-width: 0;
  }

  .dev-name {
    font-weight: 600;
  }

  .btn.small {
    min-height: 28px;
    padding: 0 var(--space-2);
    font-size: var(--text-s);
  }
</style>
