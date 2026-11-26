<script lang="ts">
  import type { Snippet } from 'svelte';
  import List from 'phosphor-svelte/lib/ListIcon';
  import Plus from 'phosphor-svelte/lib/PlusIcon';
  import Trash from 'phosphor-svelte/lib/TrashIcon';
  import X from 'phosphor-svelte/lib/XIcon';
  import { getLocale, t } from '../lib/i18n/index.svelte';
  import { chats } from './chats.svelte';

  /**
   * The top of the phone chat (ADR 0039): the button that opens the chat list
   * (a drawer from the left), the model in the middle and the red trash button
   * that clears the open chat.
   */
  let { busy = false, children }: { busy?: boolean; children?: Snippet } = $props();

  let open = $state(false);
  /** Clearing and deleting take two taps: the first asks, the second does it. */
  let confirmClear = $state(false);
  let confirmDelete = $state('');
  let timer: ReturnType<typeof setTimeout> | undefined;

  function ask(fn: () => void) {
    clearTimeout(timer);
    fn();
    timer = setTimeout(() => ((confirmClear = false), (confirmDelete = '')), 4000);
  }

  function clear() {
    if (!confirmClear) return ask(() => (confirmClear = true));
    confirmClear = false;
    chats.clear();
  }

  function remove(id: string) {
    if (confirmDelete !== id) return ask(() => (confirmDelete = id));
    confirmDelete = '';
    chats.remove(id);
  }

  function pick(id: string) {
    chats.open(id);
    open = false;
  }

  function startNew() {
    chats.startNew();
    open = false;
  }

  function day(ms: number): string {
    return new Date(ms).toLocaleDateString(getLocale(), { day: 'numeric', month: 'short' });
  }
</script>

<div class="chat-bar">
  <button
    type="button"
    class="icon-btn"
    aria-label={t('mobile.chats.open')}
    title={t('mobile.chats.open')}
    aria-expanded={open}
    onclick={() => (open = true)}
  >
    <List size={22} aria-hidden="true" />
  </button>
  <div class="middle">{@render children?.()}</div>
  {#if chats.messages.length}
    <button
      type="button"
      class={confirmClear ? 'btn btn-danger sure' : 'icon-btn danger'}
      aria-label={confirmClear ? t('mobile.chats.clearSure') : t('mobile.chats.clear')}
      title={t('mobile.chats.clear')}
      onclick={clear}
      disabled={busy}
    >
      <Trash size={20} aria-hidden="true" />{#if confirmClear}{t('mobile.chats.clearSure')}{/if}
    </button>
  {/if}
</div>

{#if open}
  <div class="backdrop" role="presentation" onclick={() => (open = false)}></div>
  <div class="drawer" role="dialog" aria-modal="true" aria-labelledby="chats-title">
    <header>
      <h2 id="chats-title">{t('mobile.chats.title')}</h2>
      <button type="button" class="icon-btn" aria-label={t('common.close')} title={t('common.close')} onclick={() => (open = false)}>
        <X size={22} aria-hidden="true" />
      </button>
    </header>
    <button type="button" class="btn btn-primary new" onclick={startNew} disabled={busy}>
      <Plus size={20} aria-hidden="true" />{t('mobile.chats.new')}
    </button>
    {#if chats.list.length === 0}
      <p class="empty">{t('mobile.chats.empty')}</p>
    {:else}
      <ul>
        {#each chats.list as c (c.id)}
          <li class:current={c.id === chats.currentId}>
            <button
              type="button"
              class="pick"
              onclick={() => pick(c.id)}
              disabled={busy}
              aria-current={c.id === chats.currentId ? 'true' : undefined}
            >
              <span class="title">{c.title || t('chat.sessions.untitled')}</span>
              <span class="when">{day(c.updated)}</span>
            </button>
            <button
              type="button"
              class={confirmDelete === c.id ? 'btn btn-danger sure' : 'icon-btn danger'}
              aria-label={confirmDelete === c.id ? t('mobile.chats.deleteSure') : t('mobile.chats.delete')}
              title={t('mobile.chats.delete')}
              onclick={() => remove(c.id)}
              disabled={busy && c.id === chats.currentId}
            >
              <Trash size={20} aria-hidden="true" />{#if confirmDelete === c.id}{t('mobile.chats.deleteSure')}{/if}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
{/if}

<style>
  .chat-bar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .middle {
    display: flex;
    flex: 1;
    min-width: 0;
  }

  .middle > :global(*) {
    flex: 1;
    min-width: 0;
  }

  .chat-bar > :global(.icon-btn),
  .chat-bar > :global(.btn) {
    min-width: 52px;
    min-height: 52px;
  }

  .sure {
    padding: 0 var(--space-3);
    white-space: nowrap;
  }

  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: var(--overlay);
  }

  .drawer {
    position: fixed;
    top: 0;
    bottom: 0;
    left: 0;
    z-index: 41;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    width: min(85vw, 340px);
    padding: max(var(--space-8), env(safe-area-inset-top, 0px)) var(--space-4)
      max(var(--space-4), env(safe-area-inset-bottom, 0px));
    border-right: 1px solid var(--border);
    background: var(--surface);
    box-shadow: var(--shadow);
    overflow-y: auto;
    animation: slide-in var(--dur-mid) var(--ease);
  }

  @keyframes slide-in {
    from {
      transform: translateX(-100%);
    }
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  h2 {
    margin: 0;
    font-size: var(--text-l);
  }

  .new {
    justify-content: center;
    min-height: 52px;
  }

  ul {
    display: grid;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    border-radius: var(--radius-m);
  }

  li.current {
    background: var(--surface-2);
  }

  .pick {
    display: grid;
    flex: 1;
    min-width: 0;
    min-height: 52px;
    padding: var(--space-2) var(--space-3);
    border: 0;
    background: none;
    color: var(--text);
    text-align: left;
  }

  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .when {
    color: var(--text-faint);
    font-size: var(--text-xs);
  }

  li :global(.icon-btn),
  li :global(.btn) {
    min-width: 48px;
    min-height: 48px;
  }

  .empty {
    color: var(--text-muted);
    font-size: var(--text-s);
  }
</style>
