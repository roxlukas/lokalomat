<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import SpeakerHigh from 'phosphor-svelte/lib/SpeakerHighIcon';
  import CaretLeft from 'phosphor-svelte/lib/CaretLeftIcon';
  import CaretRight from 'phosphor-svelte/lib/CaretRightIcon';
  import FileText from 'phosphor-svelte/lib/FileTextIcon';
  import MusicNotes from 'phosphor-svelte/lib/MusicNotesIcon';
  import ArrowClockwise from 'phosphor-svelte/lib/ArrowClockwiseIcon';
  import X from 'phosphor-svelte/lib/XIcon';
  import DownloadSimple from 'phosphor-svelte/lib/DownloadSimpleIcon';
  import ArrowCounterClockwise from 'phosphor-svelte/lib/ArrowCounterClockwiseIcon';
  import Trash from 'phosphor-svelte/lib/TrashIcon';
  import { getLocale, t } from '../lib/i18n/index.svelte';
  import { api, describe, fileUrl, type GalleryItem } from './api';
  import { fileName } from './jobs';
  import type { Reuse, ReuseKind } from './phone.svelte';

  interface Props {
    /** "Użyj ponownie": opens the mode's screen with the result's settings (ADR 0037). */
    onReuse: (r: Reuse) => void;
    /** The computer can run that mode now. */
    canReuse: (kind: ReuseKind) => boolean;
  }

  let { onReuse, canReuse }: Props = $props();

  /**
   * Gallery (V9) like the desktop one: a grid of thumbnails, newest first.
   * Thumbnails and files come through the phone's Rust side (device key,
   * pinned TLS) and are shown as object URLs that live only in this screen.
   */
  const PAGE = 60;
  const MAX = 100;
  const PARALLEL = 3;

  let items = $state<GalleryItem[]>([]);
  let limit = $state(PAGE);
  let loading = $state(false);
  let error = $state('');
  let thumbs = $state<Record<string, string>>({});
  let failed = $state<Record<string, true>>({});
  let viewer = $state<{ item: GalleryItem; index: number; url: string | null; type: string; loading: boolean } | null>(null);
  let saved = $state('');
  let confirmDelete = $state(false);
  let deleting = $state(false);

  const urls: string[] = [];
  const queue: (() => Promise<void>)[] = [];
  let running = 0;

  onMount(() => void load());
  onDestroy(() => urls.forEach((u) => URL.revokeObjectURL(u)));

  async function load() {
    loading = true;
    error = '';
    try {
      items = await api.gallery(limit);
    } catch (e) {
      error = describe(e);
    } finally {
      loading = false;
    }
  }

  function more() {
    limit = Math.min(MAX, limit + PAGE);
    void load();
  }

  function keep(url: string): string {
    urls.push(url);
    return url;
  }

  function pump() {
    while (running < PARALLEL && queue.length > 0) {
      const job = queue.shift()!;
      running += 1;
      void job().finally(() => {
        running -= 1;
        pump();
      });
    }
  }

  function thumbKey(item: GalleryItem) {
    return `${item.job_id}:0`;
  }

  /** Loads a tile's thumbnail once it scrolls into view. */
  function lazyThumb(node: HTMLElement, item: GalleryItem) {
    const key = thumbKey(item);
    if (item.kind !== 'image' || item.files === 0 || thumbs[key]) return {};
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((e) => e.isIntersecting)) return;
        observer.disconnect();
        queue.push(async () => {
          try {
            thumbs[key] = keep(fileUrl(await api.galleryThumb(item.job_id, 0)));
          } catch {
            failed[key] = true;
          }
        });
        pump();
      },
      { rootMargin: '200px' },
    );
    observer.observe(node);
    return { destroy: () => observer.disconnect() };
  }

  async function open(item: GalleryItem, index = 0) {
    viewer = { item, index, url: null, type: '', loading: item.files > 0 };
    saved = '';
    confirmDelete = false;
    if (item.files === 0) return;
    try {
      const file = await api.galleryFile(item.job_id, index);
      const url = keep(fileUrl(file));
      if (viewer?.item.job_id === item.job_id && viewer.index === index) {
        viewer.url = url;
        viewer.type = file.content_type;
      }
    } catch (e) {
      error = describe(e);
      viewer = null;
    } finally {
      if (viewer) viewer.loading = false;
    }
  }

  function step(delta: number) {
    if (!viewer) return;
    const next = viewer.index + delta;
    if (next >= 0 && next < viewer.item.files) void open(viewer.item, next);
  }

  /** Saves the shown file on the phone (the system save dialog). */
  async function save() {
    const v = viewer;
    if (!v?.url) return;
    const stems: Record<string, string> = { image: t('mobile.image.file'), music: t('jobs.songFile'), tts: t('mobile.gallery.voiceFile') };
    saved = '';
    try {
      if (await api.saveFile(v.item.job_id, v.index, fileName(stems[v.item.kind] ?? '', v.item, { content_type: v.type }))) saved = t('jobs.saved');
    } catch (e) {
      // Shown in the viewer: the gallery's own error line is under it.
      saved = describe(e);
    }
  }

  function reusable(item: GalleryItem): ReuseKind | null {
    const kind = item.kind === 'image' || item.kind === 'tts' || item.kind === 'music' ? item.kind : null;
    return kind && item.params && canReuse(kind) ? kind : null;
  }

  function reuse(item: GalleryItem) {
    const kind = reusable(item);
    if (!kind) return;
    viewer = null;
    onReuse({ kind, params: item.params ?? {} });
  }

  /** Deletes the result on the computer (after the user confirmed). */
  async function remove(item: GalleryItem) {
    deleting = true;
    saved = '';
    try {
      await api.deleteGalleryItem(item.job_id);
      items = items.filter((i) => i.job_id !== item.job_id);
      viewer = null;
    } catch (e) {
      saved = describe(e);
    } finally {
      deleting = false;
      confirmDelete = false;
    }
  }

  function when(secs: number): string {
    return new Date(secs * 1000).toLocaleString(getLocale(), { dateStyle: 'short', timeStyle: 'short' });
  }
</script>

<div class="gallery-view">
  <div class="top">
    <h2>{t('mobile.tabs.gallery')}</h2>
    <button type="button" class="icon-btn" aria-label={t('mobile.gallery.refresh')} title={t('mobile.gallery.refresh')} onclick={load} disabled={loading}>
      <ArrowClockwise size={18} aria-hidden="true" />
    </button>
  </div>

  {#if error}<p class="note note-error" role="alert">{error}</p>{/if}
  {#if items.length === 0}
    <p class="muted">{loading ? t('mobile.loading') : t('mobile.gallery.empty')}</p>
  {/if}

  <ul class="grid" aria-label={t('mobile.tabs.gallery')}>
    {#each items as item (item.job_id)}
      {@const key = thumbKey(item)}
      <li>
        <!-- A song has its WAV and MP3: open the last file, the smaller MP3. -->
        <button type="button" class="tile" onclick={() => open(item, item.kind === 'music' ? Math.max(0, item.files - 1) : 0)}>
          {#if item.kind === 'image'}
            <span class="frame" use:lazyThumb={item}>
              {#if thumbs[key]}
                <img src={thumbs[key]} alt={item.text} />
              {:else if !failed[key]}
                <span class="skeleton" aria-hidden="true"></span>
              {/if}
            </span>
            {#if item.files > 1}<span class="count">+{item.files - 1}</span>{/if}
          {:else}
            <span class="frame text-tile">
              {#if item.kind === 'tts'}<SpeakerHigh size={20} aria-hidden="true" />{:else if item.kind === 'music'}<MusicNotes size={20} aria-hidden="true" />{:else}<FileText size={20} aria-hidden="true" />{/if}
              <span class="excerpt">{item.text}</span>
            </span>
          {/if}
          <span class="caption">
            <span>{t(`mobile.gallery.kinds.${item.kind}`)}</span>
            <span class="muted">{when(item.created_at)}</span>
          </span>
        </button>
      </li>
    {/each}
  </ul>

  {#if items.length >= limit && limit < MAX}
    <button type="button" class="btn more" onclick={more} disabled={loading}>{t('mobile.gallery.more')}</button>
  {/if}
</div>

{#if viewer}
  {@const v = viewer}
  <div class="viewer" role="dialog" aria-modal="true" aria-label={t(`mobile.gallery.kinds.${v.item.kind}`)}>
    <div class="viewer-top">
      <span class="small">{t(`mobile.gallery.kinds.${v.item.kind}`)} · {when(v.item.created_at)}</span>
      <button type="button" class="icon-btn" aria-label={t('common.close')} onclick={() => (viewer = null)}>
        <X size={20} aria-hidden="true" />
      </button>
    </div>
    <div class="stage">
      {#if v.loading}
        <span class="skeleton big" aria-hidden="true"></span>
      {:else if v.item.kind === 'image' && v.url}
        <img src={v.url} alt={v.item.text} />
      {:else if (v.item.kind === 'tts' || v.item.kind === 'music') && v.url}
        <audio controls src={v.url}></audio>
      {/if}
      {#if v.item.files > 1 && v.item.kind !== 'music'}
        <div class="pager">
          <button type="button" class="icon-btn" aria-label={t('mobile.gallery.prev')} onclick={() => step(-1)} disabled={v.index === 0}>
            <CaretLeft size={22} aria-hidden="true" />
          </button>
          <span class="small">{v.index + 1} / {v.item.files}</span>
          <button type="button" class="icon-btn" aria-label={t('mobile.gallery.next')} onclick={() => step(1)} disabled={v.index >= v.item.files - 1}>
            <CaretRight size={22} aria-hidden="true" />
          </button>
        </div>
      {/if}
    </div>
    <div class="actions">
      {#if v.url}
        <button type="button" class="btn" onclick={save}><DownloadSimple size={18} aria-hidden="true" />{t('mobile.save')}</button>
      {/if}
      {#if reusable(v.item)}
        <button type="button" class="btn" onclick={() => reuse(v.item)}><ArrowCounterClockwise size={18} aria-hidden="true" />{t('mobile.gallery.reuse')}</button>
      {/if}
      <button type="button" class="btn danger" onclick={() => (confirmDelete = true)} disabled={deleting}><Trash size={18} aria-hidden="true" />{t('mobile.gallery.delete')}</button>
    </div>
    {#if confirmDelete}
      <div class="confirm" role="alertdialog" aria-labelledby="delete-question">
        <p id="delete-question">{t('mobile.gallery.deleteConfirm')}</p>
        <div class="actions">
          <button type="button" class="btn" onclick={() => (confirmDelete = false)} disabled={deleting}>{t('common.cancel')}</button>
          <button type="button" class="btn btn-danger" onclick={() => remove(v.item)} disabled={deleting}>{t('mobile.gallery.delete')}</button>
        </div>
      </div>
    {/if}
    {#if saved}<p class="small" role="status">{saved}</p>{/if}
    {#if v.item.text}<p class="prompt">{v.item.text}</p>{/if}
    <p class="small muted">{v.item.model}</p>
  </div>
{/if}

<style>
  .gallery-view {
    display: grid;
    gap: var(--space-3);
  }

  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  h2 {
    font-size: var(--text-l);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: var(--space-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    position: relative;
  }

  .tile {
    display: grid;
    width: 100%;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    background: var(--surface);
    color: inherit;
    overflow: hidden;
    text-align: left;
  }

  .tile:active {
    border-color: var(--accent);
  }

  .frame {
    display: block;
    width: 100%;
    aspect-ratio: 1;
    background: var(--surface-2);
  }

  .frame img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .text-tile {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    color: var(--text-muted);
  }

  .excerpt {
    display: -webkit-box;
    overflow: hidden;
    color: var(--text);
    font-size: var(--text-s);
    -webkit-line-clamp: 5;
    line-clamp: 5;
    -webkit-box-orient: vertical;
  }

  .skeleton {
    display: block;
    width: 100%;
    height: 100%;
    background: linear-gradient(90deg, var(--surface-2), var(--surface-3), var(--surface-2));
    background-size: 200% 100%;
    animation: shimmer 1.4s ease-in-out infinite;
  }

  .skeleton.big {
    aspect-ratio: 1;
    border-radius: var(--radius-m);
  }

  @keyframes shimmer {
    from {
      background-position: 100% 0;
    }
    to {
      background-position: -100% 0;
    }
  }

  .count {
    position: absolute;
    top: var(--space-2);
    right: var(--space-2);
    padding: 0 6px;
    border-radius: var(--radius-pill);
    background: var(--surface);
    font-size: var(--text-xs);
  }

  .caption {
    display: flex;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2);
    font-size: var(--text-xs);
  }

  .more {
    justify-self: center;
  }

  .viewer {
    position: fixed;
    inset: 0;
    z-index: 10;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: max(var(--space-8), env(safe-area-inset-top, 0px)) var(--space-4) var(--space-6);
    background: var(--bg);
    overflow-y: auto;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .confirm {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--error);
    border-radius: var(--radius-m);
    background: var(--surface);
  }

  .viewer-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .stage {
    display: grid;
    gap: var(--space-2);
    justify-items: center;
  }

  .stage img {
    max-width: 100%;
    max-height: 70dvh;
    border-radius: var(--radius-m);
    object-fit: contain;
  }

  .stage audio {
    width: 100%;
  }

  .pager {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .prompt {
    white-space: pre-wrap;
  }

  .small {
    font-size: var(--text-s);
  }

  .muted {
    color: var(--text-muted);
  }
</style>
