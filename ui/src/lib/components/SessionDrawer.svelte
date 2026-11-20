<script lang="ts">
  import Plus from '@lucide/svelte/icons/plus';
  import Search from '@lucide/svelte/icons/search';
  import Pin from '@lucide/svelte/icons/pin';
  import PinOff from '@lucide/svelte/icons/pin-off';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import { getLocale, t } from '../i18n/index.svelte';
  import { chat } from '../chat.svelte';
  import { describeError } from '../feedback';

  let editing = $state<number | null>(null);
  let draft = $state('');
  let confirming = $state<number | null>(null);
  let error = $state('');
  let timer: ReturnType<typeof setTimeout> | null = null;

  function onSearch() {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => void chat.loadSessions(), 200);
  }

  async function act(fn: () => Promise<void>) {
    error = '';
    try {
      await fn();
    } catch (e) {
      error = describeError(e);
    }
  }

  async function saveTitle(id: number) {
    const title = draft.trim();
    editing = null;
    if (title) await act(() => chat.rename(id, title));
  }

  function day(ts: number): string {
    return new Date(ts * 1000).toLocaleDateString(getLocale(), { day: 'numeric', month: 'short' });
  }
</script>

<aside class="drawer" aria-label={t('chat.sessions.title')}>
  <button type="button" class="btn new" onclick={() => chat.startNew()}>
    <Plus size={16} aria-hidden="true" />{t('chat.sessions.new')}
  </button>
  <label class="search">
    <Search size={14} aria-hidden="true" />
    <span class="sr-only">{t('chat.sessions.search')}</span>
    <input type="search" placeholder={t('chat.sessions.search')} bind:value={chat.query} oninput={onSearch} />
  </label>
  {#if error}<p class="note note-error">{error}</p>{/if}

  {#if chat.sessions.length === 0}
    <p class="empty">{chat.query ? t('chat.sessions.noResults') : t('chat.sessions.empty')}</p>
  {:else}
    <ul>
      {#each chat.sessions as s (s.id)}
        <li class:current={s.id === chat.currentId}>
          {#if editing === s.id}
            <form onsubmit={(e) => (e.preventDefault(), saveTitle(s.id))}>
              <!-- svelte-ignore a11y_autofocus -->
              <input
                class="rename"
                bind:value={draft}
                autofocus
                aria-label={t('chat.sessions.rename')}
                onblur={() => saveTitle(s.id)}
                onkeydown={(e) => e.key === 'Escape' && (editing = null)}
              />
            </form>
          {:else}
            <button type="button" class="open" onclick={() => act(() => chat.open(s.id))} aria-current={s.id === chat.currentId ? 'true' : undefined}>
              <span class="title">{#if s.pinned}<Pin size={12} aria-label={t('chat.sessions.pinned')} />{/if}{s.title || t('chat.sessions.untitled')}</span>
              <span class="when">{day(s.updated_at)}</span>
            </button>
            <span class="actions">
              {#if confirming === s.id}
                <button type="button" class="btn tiny danger" onclick={() => act(() => chat.remove(s.id))}>{t('chat.sessions.confirmDelete')}</button>
                <button type="button" class="btn tiny" onclick={() => (confirming = null)}>{t('common.cancel')}</button>
              {:else}
                <button type="button" class="icon-btn sm" title={s.pinned ? t('chat.sessions.unpin') : t('chat.sessions.pin')}
                  aria-label={s.pinned ? t('chat.sessions.unpin') : t('chat.sessions.pin')} onclick={() => act(() => chat.pin(s.id, !s.pinned))}>
                  {#if s.pinned}<PinOff size={14} aria-hidden="true" />{:else}<Pin size={14} aria-hidden="true" />{/if}
                </button>
                <button type="button" class="icon-btn sm" title={t('chat.sessions.rename')} aria-label={t('chat.sessions.rename')}
                  onclick={() => ((editing = s.id), (draft = s.title))}><Pencil size={14} aria-hidden="true" /></button>
                <button type="button" class="icon-btn sm" title={t('chat.sessions.delete')} aria-label={t('chat.sessions.delete')}
                  onclick={() => (confirming = s.id)}><Trash2 size={14} aria-hidden="true" /></button>
              {/if}
            </span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</aside>

<style>
  .drawer {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    width: 260px;
    padding: var(--space-3);
    border-right: 1px solid var(--border);
    background: var(--surface);
    overflow-y: auto;
  }

  .new {
    justify-content: center;
  }

  .search {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
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
    font-size: var(--text-s);
  }

  .search:focus-within {
    outline: 2px solid var(--focus);
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    position: relative;
    border-radius: var(--radius-s);
  }

  li:hover,
  li.current {
    background: var(--surface-2);
  }

  .open {
    display: flex;
    flex-direction: column;
    width: 100%;
    padding: var(--space-2);
    border: 0;
    background: none;
    text-align: left;
  }

  .title {
    display: flex;
    align-items: center;
    gap: 4px;
    overflow: hidden;
    font-size: var(--text-s);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .when {
    color: var(--text-faint);
    font-size: var(--text-xs);
  }

  .actions {
    position: absolute;
    top: 50%;
    right: 4px;
    display: none;
    gap: 2px;
    transform: translateY(-50%);
    background: var(--surface-2);
  }

  li:hover .actions,
  li:focus-within .actions {
    display: flex;
  }

  .icon-btn.sm {
    width: 26px;
    height: 26px;
  }

  .btn.tiny {
    min-height: 24px;
    padding: 0 var(--space-2);
    font-size: var(--text-xs);
  }

  .btn.danger {
    border-color: var(--error);
    color: var(--error);
  }

  .rename {
    width: 100%;
    height: 32px;
    padding: 0 var(--space-2);
    border: 1px solid var(--focus);
    border-radius: var(--radius-s);
    background: var(--surface);
  }

  .empty {
    padding: var(--space-4) var(--space-2);
    color: var(--text-muted);
    font-size: var(--text-s);
  }
</style>
