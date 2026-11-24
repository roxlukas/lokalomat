<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import DownloadSimple from 'phosphor-svelte/lib/DownloadSimpleIcon';
  import { t } from '../lib/i18n/index.svelte';
  import { readLocal, writeLocal } from '../lib/storage';
  import { api, describe, fileUrl, finished, type JobView } from './api';
  import { download, fileName, pickFile, rememberJob, rememberedJob, watchJob, type Watch } from './jobs';
  import JobProgress from './JobProgress.svelte';
  import ModelPicker from './ModelPicker.svelte';

  /** Music mode (ADR 0022): a song made on the computer, played and saved on the phone. */
  const SLOT = 'music';
  const LANGUAGES = ['pl', 'en', 'de', 'fr', 'es', 'it', 'uk'];
  /** Song lengths in seconds; 0 = the model decides. */
  const LENGTHS = [0, 30, 60, 120, 180];
  const MAX_CAPTION = 2000;
  const MAX_LYRICS = 10000;
  // The MP3 is smaller to send over Wi-Fi; the WAV is the fallback.
  const PREFERRED = 'audio/mpeg';

  let caption = $state(readLocal<string>('lokalsi.mobile.music.caption', ''));
  let lyrics = $state(readLocal<string>('lokalsi.mobile.music.lyrics', ''));
  let language = $state(readLocal<string>('lokalsi.mobile.music.language', 'pl'));
  let length = $state(readLocal<number>('lokalsi.mobile.music.length', 60));
  let model = $state('');
  let job = $state<JobView | null>(null);
  let url = $state<string | null>(null);
  let error = $state('');
  let note = $state('');
  let starting = $state(false);
  let watch: Watch | undefined;

  let working = $derived(starting || (job !== null && !finished(job)));

  $effect(() => writeLocal('lokalsi.mobile.music.caption', caption));
  $effect(() => writeLocal('lokalsi.mobile.music.lyrics', lyrics));
  $effect(() => writeLocal('lokalsi.mobile.music.language', language));
  $effect(() => writeLocal('lokalsi.mobile.music.length', length));

  onMount(() => {
    const id = rememberedJob(SLOT);
    if (id !== null) follow(id);
  });

  onDestroy(() => {
    watch?.stop();
    if (url) URL.revokeObjectURL(url);
  });

  function follow(id: number) {
    watch?.stop();
    watch = watchJob(
      id,
      (j) => {
        job = j;
        if (j.status === 'done') void show(j);
      },
      (e) => {
        rememberJob(SLOT, null);
        job = null;
        if ((e as { code?: string })?.code !== 'not_found') error = describe(e);
      },
    );
  }

  async function show(j: JobView) {
    const file = pickFile(j, PREFERRED);
    if (!file) return;
    try {
      const next = fileUrl(await download(j, file.index));
      if (url) URL.revokeObjectURL(url);
      url = next;
    } catch (e) {
      error = describe(e);
    }
  }

  async function compose() {
    error = '';
    note = '';
    starting = true;
    try {
      const started = await api.startMusic(caption.trim(), lyrics.trim(), language, length > 0 ? length : null, model || null);
      if (url) URL.revokeObjectURL(url);
      url = null;
      job = started;
      rememberJob(SLOT, started.job_id);
      follow(started.job_id);
    } catch (e) {
      error = describe(e);
    } finally {
      starting = false;
    }
  }

  async function cancel() {
    if (!job) return;
    try {
      job = await api.cancelJob(job.job_id);
    } catch (e) {
      error = describe(e);
    }
  }

  async function save() {
    const file = job && pickFile(job, PREFERRED);
    if (!job || !file) return;
    note = '';
    error = '';
    try {
      if (await api.saveFile(job.job_id, file.index, fileName(t('jobs.songFile'), job, file))) note = t('jobs.saved');
    } catch (e) {
      error = describe(e);
    }
  }

  function lengthLabel(s: number): string {
    if (s === 0) return t('workshop.music.durationAuto');
    return s < 60 ? t('workshop.music.seconds', { count: s }) : t('workshop.music.minutes', { count: s / 60 });
  }
</script>

<div class="panel">
  {#if error}<p class="note note-error" role="alert">{error}</p>{/if}
  <ModelPicker mode="music" bind:value={model} />
  <textarea rows="2" bind:value={caption} maxlength={MAX_CAPTION} placeholder={t('workshop.music.placeholder')} aria-label={t('workshop.music.placeholder')}></textarea>
  <label class="field">
    <span class="small muted">{t('workshop.music.lyrics')}</span>
    <textarea rows="5" bind:value={lyrics} maxlength={MAX_LYRICS} placeholder={t('workshop.music.lyricsPlaceholder')}></textarea>
  </label>
  <div class="row">
    <label class="field">
      <span class="small muted">{t('workshop.music.language')}</span>
      <select bind:value={language}>
        {#each LANGUAGES as l (l)}<option value={l}>{t(`workshop.music.languages.${l}`)}</option>{/each}
      </select>
    </label>
    <div class="field">
      <span class="small muted" id="song-length">{t('workshop.music.duration')}</span>
      <div class="chips" role="radiogroup" aria-labelledby="song-length">
        {#each LENGTHS as s (s)}
          <button type="button" role="radio" aria-checked={length === s} class="chip" class:active={length === s} onclick={() => (length = s)}>{lengthLabel(s)}</button>
        {/each}
      </div>
    </div>
  </div>
  <button type="button" class="btn btn-primary" onclick={compose} disabled={working || (!caption.trim() && !lyrics.trim())}>{t('workshop.music.compose')}</button>
  {#if job}<JobProgress {job} onCancel={cancel} />{/if}
  {#if url && job?.status === 'done'}
    <audio controls src={url}></audio>
    <button type="button" class="btn" onclick={save}><DownloadSimple size={18} aria-hidden="true" />{t('mobile.save')}</button>
    {#if note}<p class="muted small" role="status">{note}</p>{/if}
  {/if}
  <p class="muted small">{t('workshop.music.note')}</p>
</div>

<style>
  .panel {
    display: grid;
    gap: var(--space-3);
  }

  textarea,
  select {
    width: 100%;
  }

  .field {
    display: grid;
    gap: var(--space-1);
  }

  .row {
    display: grid;
    gap: var(--space-3);
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .chip {
    min-height: 32px;
    padding: 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
    background: var(--surface-2);
    color: var(--text);
  }

  .chip.active {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  audio {
    width: 100%;
  }

  .small {
    font-size: var(--text-s);
  }

  .muted {
    color: var(--text-muted);
  }
</style>
