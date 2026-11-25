<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import ArrowCounterClockwise from 'phosphor-svelte/lib/ArrowCounterClockwiseIcon';
  import Check from 'phosphor-svelte/lib/CheckIcon';
  import Cpu from 'phosphor-svelte/lib/CpuIcon';
  import DownloadSimple from 'phosphor-svelte/lib/DownloadSimpleIcon';
  import PaperPlaneRight from 'phosphor-svelte/lib/PaperPlaneRightIcon';
  import Stop from 'phosphor-svelte/lib/StopIcon';
  import Trash from 'phosphor-svelte/lib/TrashIcon';
  import { getLocale, t } from '../lib/i18n/index.svelte';
  import { renderMarkdown } from '../lib/markdown';
  import { readLocal, writeLocal } from '../lib/storage';
  import { describe, type Turn } from './api';
  import { source } from './source.svelte';
  import Spinner from './Spinner.svelte';
  import { gigabytes, local, percent, type PhoneModel, type PhoneModels, type Progress } from './local';

  /** Chat with a model that runs on the phone itself (ADR 0038): no computer, no network. */
  const CHOSEN_KEY = 'lokalsi.mobile.local.model';

  let info = $state<PhoneModels | null>(null);
  let chosen = $state(readLocal<string>(CHOSEN_KEY, ''));
  let loadedId = '';
  let managing = $state(false);
  let messages = $state<Turn[]>([]);
  let input = $state('');
  let error = $state('');
  /** What the app is doing while the reply is not there yet. */
  let phase = $state<'idle' | 'loading' | 'thinking' | 'writing'>('idle');
  let busy = $state(false);
  let progress = $state<Record<string, Progress>>({});
  /** Deleting takes two taps: the first asks, the second deletes. */
  let confirmDelete = $state('');

  const installed = $derived(info?.models.filter((m) => m.installed) ?? []);
  const current = $derived(installed.find((m) => m.id === chosen) ?? installed[0]);

  onMount(refresh);

  // Settings, "Manage models": open the list (this screen may already be on display).
  $effect(() => {
    if (source.openModels) {
      source.openModels = false;
      managing = true;
      void refresh();
    }
  });

  // The loaded model takes a few GB of memory: free it when this screen goes away.
  onDestroy(() => {
    void local.cancel().catch(() => undefined);
    void local.unload().catch(() => undefined);
  });

  async function refresh() {
    try {
      info = await local.models();
      const ready = info.models.filter((m) => m.installed);
      if (!ready.some((m) => m.id === chosen)) chosen = ready[0]?.id ?? '';
      // Nothing to chat with yet: start on the list of models.
      if (ready.length === 0) managing = true;
    } catch (e) {
      error = describe(e);
    }
  }

  function description(m: PhoneModel): string {
    return getLocale() === 'pl' ? m.description_pl : m.description_en;
  }

  function use(m: PhoneModel) {
    chosen = m.id;
    writeLocal(CHOSEN_KEY, m.id);
    managing = false;
  }

  async function download(m: PhoneModel) {
    error = '';
    progress[m.id] = { downloaded: m.partial_bytes, total: m.size };
    try {
      await local.download(m.id, (p) => (progress[m.id] = p));
      await refresh();
      const fresh = info?.models.find((x) => x.id === m.id);
      if (fresh?.installed && installed.length <= 1) use(fresh);
    } catch (e) {
      const code = typeof e === 'object' && e && 'code' in e ? String((e as { code: string }).code) : '';
      // The user cancelled: not an error.
      if (code !== 'download_cancelled') error = describe(e);
      await refresh();
    } finally {
      delete progress[m.id];
    }
  }

  async function remove(m: PhoneModel) {
    error = '';
    if (confirmDelete !== m.id) {
      confirmDelete = m.id;
      return;
    }
    confirmDelete = '';
    try {
      if (loadedId === m.id) {
        await local.unload();
        loadedId = '';
      }
      await local.remove(m.id);
      await refresh();
    } catch (e) {
      error = describe(e);
    }
  }

  async function send() {
    const text = input.trim();
    if (!text || busy || !current) return;
    error = '';
    input = '';
    busy = true;
    const history = [...messages];
    messages = [...messages, { role: 'user', content: text }, { role: 'assistant', content: '' }];
    const answer = messages.length - 1;
    try {
      if (loadedId !== current.id) {
        phase = 'loading';
        loadedId = '';
        await local.load(current.id);
        loadedId = current.id;
      }
      phase = 'thinking';
      await local.generate(
        text,
        history,
        (piece) => {
          phase = 'writing';
          const bubble = messages[answer];
          if (bubble) bubble.content += piece;
        },
        () => (phase = 'thinking'),
      );
    } catch (e) {
      error = describe(e);
    } finally {
      phase = 'idle';
      busy = false;
      // A reply that never started leaves an empty bubble: drop it.
      if (!messages[answer]?.content) messages = messages.slice(0, answer);
    }
  }

  function stop() {
    void local.cancel().catch(() => undefined);
  }

  function reset() {
    messages = [];
    error = '';
  }
</script>

{#if managing || !current}
  <section class="models" aria-labelledby="models-title">
    <h2 id="models-title">{t('mobile.local.chooseTitle')}</h2>
    <p class="muted small">{t('mobile.local.chooseBody')}</p>
    {#if error}<p class="note note-error" role="alert">{error}</p>{/if}
    {#each info?.models ?? [] as m (m.id)}
      {@const p = progress[m.id]}
      <article class="card" class:active={current?.id === m.id}>
        <div class="head">
          <strong>{m.name}</strong>
          {#if m.recommended}<span class="badge">{t('mobile.local.recommended')}</span>{/if}
        </div>
        <p class="small muted">{description(m)} · {gigabytes(m.size)}</p>
        {#if m.fit === 'low'}<p class="note note-warn small">{t('mobile.local.low')}</p>{/if}
        {#if p}
          <progress max="100" value={percent(p)} aria-label={t('mobile.local.downloading', { percent: percent(p) })}></progress>
          <p class="small muted">{t('mobile.local.downloading', { percent: percent(p) })}</p>
          <button type="button" class="btn big" onclick={() => void local.cancelDownload(m.id)}>{t('common.cancel')}</button>
        {:else if m.installed}
          {#if current?.id === m.id}
            <p class="inuse"><Check size={18} aria-hidden="true" />{t('mobile.local.inUse')}</p>
          {:else}
            <button type="button" class="btn btn-primary big" onclick={() => use(m)}>{t('mobile.local.use')}</button>
          {/if}
          <button type="button" class="btn big" class:sure={confirmDelete === m.id} onclick={() => void remove(m)}>
            <Trash size={20} aria-hidden="true" />{confirmDelete === m.id ? t('mobile.local.deleteSure') : t('mobile.local.deleteFree', { size: gigabytes(m.size) })}
          </button>
        {:else}
          <p class="small muted">{t('mobile.local.wifi')}</p>
          <button type="button" class="btn btn-primary big" onclick={() => void download(m)}>
            <DownloadSimple size={20} aria-hidden="true" />{m.partial_bytes > 0 ? t('mobile.local.resume', { size: gigabytes(m.size - m.partial_bytes) }) : t('mobile.local.download', { size: gigabytes(m.size) })}
          </button>
        {/if}
      </article>
    {/each}
    {#if current}
      <button type="button" class="btn big" onclick={() => (managing = false)}>{t('common.close')}</button>
    {/if}
  </section>
{:else}
  <div class="top">
    <button type="button" class="btn model big" onclick={() => (managing = true)} title={t('mobile.local.change')}>
      <Cpu size={18} aria-hidden="true" />{current.name}
    </button>
    {#if messages.length}
      <button type="button" class="icon-btn" aria-label={t('mobile.local.newChat')} title={t('mobile.local.newChat')} onclick={reset} disabled={busy}>
        <ArrowCounterClockwise size={18} aria-hidden="true" />
      </button>
    {/if}
  </div>
  {#if error}<p class="note note-error" role="alert">{error}</p>{/if}
  <div class="stream" aria-live="polite">
    {#each messages as m, i (i)}
      <!-- an empty reply is shown as the status line below, not as an empty bubble -->
      {#if m.role !== 'assistant' || m.content}
        <div class="msg {m.role}">
          <!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitised by renderMarkdown (DOMPurify) -->
          {#if m.role === 'assistant'}{@html renderMarkdown(m.content)}{:else}{m.content}{/if}
        </div>
      {/if}
    {/each}
    {#if phase === 'loading' || phase === 'thinking'}
      <p class="status" role="status">
        <Spinner />{phase === 'loading' ? t('mobile.local.loading') : t('mobile.local.thinking')}
      </p>
    {/if}
  </div>
  <form class="bar" onsubmit={(e) => (e.preventDefault(), void send())}>
    <textarea rows="3" bind:value={input} placeholder={t('mobile.chat.placeholder')} aria-label={t('mobile.chat.placeholder')}></textarea>
    {#if busy && phase !== 'loading'}
      <button type="button" class="btn" onclick={stop} aria-label={t('mobile.local.stop')} title={t('mobile.local.stop')}>
        <Stop size={18} aria-hidden="true" />
      </button>
    {:else}
      <button type="submit" class="btn btn-primary" disabled={busy || !input.trim()} aria-label={t('mobile.send')}>
        <PaperPlaneRight size={18} aria-hidden="true" />
      </button>
    {/if}
  </form>
{/if}

<style>
  .muted {
    color: var(--text-muted);
  }

  .small {
    font-size: var(--text-s);
  }

  .models {
    display: grid;
    gap: var(--space-3);
  }

  .models h2 {
    font-size: var(--text-l);
  }

  .card {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    background: var(--surface);
  }

  .card.active {
    border-color: var(--accent);
  }

  .head,
  .row,
  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .big {
    justify-content: center;
    width: 100%;
    min-height: 52px;
    font-size: var(--text-m, 1rem);
  }

  .top .big {
    width: auto;
    flex: 1;
    justify-content: flex-start;
  }

  .sure {
    border-color: var(--error);
    color: var(--error);
    font-weight: 700;
  }

  .inuse {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-height: 52px;
    color: var(--ok);
    font-weight: 600;
  }

  .top :global(.icon-btn) {
    min-width: 52px;
    min-height: 52px;
  }

  .bar :global(.btn) {
    min-width: 52px;
    min-height: 52px;
  }

  .badge {
    padding: 2px var(--space-2);
    border-radius: var(--radius-pill);
    background: var(--accent-soft);
    color: var(--accent);
    font-size: var(--text-xs);
    font-weight: 600;
  }

  progress {
    width: 100%;
  }

  .card p,
  .models p {
    margin: 0;
  }

  .model {
    min-width: 0;
  }

  .stream {
    display: grid;
    flex: 1;
    align-content: start;
    gap: var(--space-2);
  }

  .msg {
    max-width: 90%;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-l);
    overflow-wrap: anywhere;
  }

  .msg.user {
    justify-self: end;
    background: var(--accent-soft);
    white-space: pre-wrap;
  }

  .msg.assistant {
    justify-self: start;
    background: var(--surface);
    border: 1px solid var(--border);
  }

  .status {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    color: var(--text-muted);
    font-size: var(--text-s);
  }

  .bar {
    display: flex;
    gap: var(--space-2);
    align-items: flex-end;
  }

  .bar textarea {
    flex: 1;
    width: 100%;
  }
</style>
