<script lang="ts">
  import ChevronsLeft from '@lucide/svelte/icons/chevrons-left';
  import ChevronsRight from '@lucide/svelte/icons/chevrons-right';
  import { t } from '../i18n/index.svelte';
  import { router } from '../router.svelte';
  import { readLocal, writeLocal } from '../storage';
  import { visibleViews, type ViewDef } from '../views';
  import { settings } from '../settings.svelte';

  const STORAGE_KEY = 'lokalsi.rail-expanded';
  let expanded = $state(readLocal(STORAGE_KEY, false));
  let list: HTMLElement | undefined = $state();

  let top = $derived(visibleViews(settings.value.expert).filter((v) => !v.bottom));
  let bottom = $derived(visibleViews(settings.value.expert).filter((v) => v.bottom));

  function toggle() {
    expanded = !expanded;
    writeLocal(STORAGE_KEY, expanded);
  }

  // Roving focus with arrow keys across all rail buttons.
  function onKeydown(event: KeyboardEvent) {
    if (!list) return;
    const buttons = Array.from(list.querySelectorAll<HTMLButtonElement>('button[data-nav]'));
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (index < 0) return;
    let next = index;
    if (event.key === 'ArrowDown') next = (index + 1) % buttons.length;
    else if (event.key === 'ArrowUp') next = (index - 1 + buttons.length) % buttons.length;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = buttons.length - 1;
    else return;
    event.preventDefault();
    buttons[next]?.focus();
  }
</script>

{#snippet item(view: ViewDef)}
  {@const active = router.current === view.id}
  <li>
    <button
      type="button"
      data-nav
      class="rail-item"
      class:active
      aria-current={active ? 'page' : undefined}
      aria-label={expanded ? undefined : t(view.labelKey)}
      data-tip={expanded ? undefined : t(view.labelKey)}
      onclick={() => router.go(view.id)}
      onkeydown={onKeydown}
    >
      <view.icon size={20} aria-hidden="true" />
      {#if expanded}<span class="label">{t(view.labelKey)}</span>{/if}
    </button>
  </li>
{/snippet}

<nav class="rail" class:expanded aria-label={t('nav.label')} bind:this={list}>
  <ul class="group">
    {#each top as view (view.id)}{@render item(view)}{/each}
  </ul>
  <ul class="group bottom">
    {#each bottom as view (view.id)}{@render item(view)}{/each}
    <li>
      <button
        type="button"
        class="rail-item toggle"
        aria-expanded={expanded}
        aria-label={expanded ? t('nav.collapse') : t('nav.expand')}
        data-tip={expanded ? undefined : t('nav.expand')}
        onclick={toggle}
      >
        {#if expanded}
          <ChevronsLeft size={18} aria-hidden="true" />
          <span class="label">{t('nav.collapse')}</span>
        {:else}
          <ChevronsRight size={18} aria-hidden="true" />
        {/if}
      </button>
    </li>
  </ul>
</nav>

<style>
  .rail {
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    width: var(--rail-w);
    padding: var(--space-2);
    border-right: 1px solid var(--border);
    background: var(--surface);
    transition: width var(--dur-mid) var(--ease);
    overflow-y: auto;
    overflow-x: visible;
  }

  .rail.expanded {
    width: var(--rail-w-expanded);
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .rail-item {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    min-height: 42px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius-m);
    background: transparent;
    color: var(--text-muted);
    text-align: left;
    white-space: nowrap;
  }

  .rail-item:hover {
    background: var(--surface-2);
    color: var(--text);
  }

  .rail-item.active {
    background: var(--surface-3);
    color: var(--text);
  }

  .rail-item.active::before {
    content: '';
    position: absolute;
    left: -8px;
    top: 10px;
    bottom: 10px;
    width: 3px;
    border-radius: 0 3px 3px 0;
    background: var(--accent);
  }

  .toggle {
    color: var(--text-faint);
  }

  .label {
    font-size: var(--text-m);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* tooltip for the collapsed rail, shown on hover and keyboard focus */
  .rail-item[data-tip]:hover::after,
  .rail-item[data-tip]:focus-visible::after {
    content: attr(data-tip);
    position: fixed;
    margin-left: calc(var(--rail-w) - var(--space-2));
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-s);
    background: var(--accent);
    color: var(--accent-contrast);
    font-size: var(--text-s);
    pointer-events: none;
    z-index: 20;
  }

  @media (max-width: 640px) {
    .rail,
    .rail.expanded {
      width: var(--rail-w);
    }
    .label {
      display: none;
    }
  }
</style>
