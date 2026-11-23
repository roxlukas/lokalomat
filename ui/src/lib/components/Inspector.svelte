<script lang="ts">
  import PanelRightOpen from '@lucide/svelte/icons/panel-right-open';
  import PanelRightClose from '@lucide/svelte/icons/panel-right-close';
  import { inspectorSection } from '../edition/registry';
  import { t } from '../i18n/index.svelte';
  import { router } from '../router.svelte';
  import { readLocal, writeLocal } from '../storage';
  import { workshop } from '../workshop.svelte';
  import ViewHost from './ViewHost.svelte';

  const STORAGE_KEY = 'lokalsi.inspector-open';
  let open = $state(readLocal(STORAGE_KEY, false));
  // The open Workshop mode's section, registered by the edition (ADR 0036).
  const section = $derived(router.current === 'workshop' ? inspectorSection(workshop.mode) : undefined);

  function toggle() {
    open = !open;
    writeLocal(STORAGE_KEY, open);
  }
</script>

<aside class="inspector" class:open aria-label={t('inspector.title')}>
  <button
    type="button"
    class="icon-btn toggle"
    aria-expanded={open}
    aria-controls="inspector-body"
    aria-label={open ? t('inspector.close') : t('inspector.open')}
    title={open ? t('inspector.close') : t('inspector.open')}
    onclick={toggle}
  >
    {#if open}<PanelRightClose size={18} aria-hidden="true" />{:else}<PanelRightOpen size={18} aria-hidden="true" />{/if}
  </button>
  <div id="inspector-body" class="body" hidden={!open}>
    <h2>{t('inspector.title')}</h2>
    {#if section}
      <ViewHost load={section.load} />
    {:else}
      <p class="empty">{t('inspector.empty')}</p>
    {/if}
  </div>
</aside>

<style>
  .inspector {
    position: relative;
    display: flex;
    flex-direction: column;
    width: 52px;
    border-left: 1px solid var(--border);
    background: var(--surface);
    transition: width var(--dur-mid) var(--ease);
  }

  .inspector.open {
    width: var(--inspector-w);
  }

  .toggle {
    margin: var(--space-2) var(--space-2) 0 auto;
  }

  .body {
    padding: var(--space-2) var(--space-5) var(--space-5);
    overflow-y: auto;
  }

  h2 {
    font-size: var(--text-l);
  }

  .empty {
    margin-top: var(--space-2);
    color: var(--text-muted);
    font-size: var(--text-s);
  }

  @media (max-width: 960px) {
    .inspector.open {
      position: absolute;
      right: 0;
      top: 0;
      bottom: 0;
      box-shadow: var(--shadow);
      z-index: 10;
    }
  }

  @media (max-width: 640px) {
    .inspector:not(.open) {
      width: 44px;
    }
    .inspector.open {
      width: min(var(--inspector-w), 100vw);
    }
  }
</style>
