<script lang="ts">
  import Desktop from 'phosphor-svelte/lib/DesktopIcon';
  import DeviceMobile from 'phosphor-svelte/lib/DeviceMobileIcon';
  import BrandMark from '../lib/components/BrandMark.svelte';
  import { phone } from './phone.svelte';
  import { t } from '../lib/i18n/index.svelte';

  /** First start: how will this phone be used? Two big choices (ADR 0038). */
  let { onPhone, onComputer }: { onPhone: () => void; onComputer: () => void } = $props();
</script>

<main class="welcome">
  <div class="brand">
    <BrandMark size={28} pro={phone.pro} />
    <span>{t('app.shortName')}</span>
  </div>
  <h1>{t('mobile.welcome.title')}</h1>
  <button type="button" class="tile" onclick={onPhone}>
    <DeviceMobile size={40} aria-hidden="true" />
    <strong>{t('mobile.welcome.phoneTitle')}</strong>
    <span class="muted">{t('mobile.welcome.phoneBody')}</span>
  </button>
  <button type="button" class="tile" onclick={onComputer}>
    <Desktop size={40} aria-hidden="true" />
    <strong>{t('mobile.welcome.pairTitle')}</strong>
    <span class="muted">{t('mobile.welcome.pairBody')}</span>
  </button>
</main>

<style>
  .welcome {
    display: grid;
    align-content: center;
    gap: var(--space-4);
    min-height: 100%;
    padding: var(--space-6) var(--space-4) max(var(--space-6), env(safe-area-inset-bottom, 0px));
  }

  .brand {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--accent);
  }

  .brand span {
    color: var(--text);
    font-family: var(--font-display);
    font-size: var(--text-l);
    font-weight: 700;
  }

  h1 {
    font-size: var(--text-xl, 1.5rem);
  }

  .tile {
    display: grid;
    justify-items: start;
    gap: var(--space-2);
    min-height: 160px;
    padding: var(--space-5) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-l);
    background: var(--surface);
    color: var(--accent);
    text-align: left;
  }

  .tile:active {
    border-color: var(--accent);
  }

  .tile strong {
    color: var(--text);
    font-size: var(--text-l);
  }

  .muted {
    color: var(--text-muted);
  }
</style>
