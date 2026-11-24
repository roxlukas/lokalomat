<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import DownloadSimple from 'phosphor-svelte/lib/DownloadSimpleIcon';
  import { t } from '../lib/i18n/index.svelte';
  import { readLocal, writeLocal } from '../lib/storage';
  import { api, describe, fileUrl, finished, type JobView, type RemoteVoice } from './api';
  import { download, fileName, pickFile, rememberJob, rememberedJob, watchJob, type Watch } from './jobs';
  import JobProgress from './JobProgress.svelte';
  import ModelPicker from './ModelPicker.svelte';
  import { num, phone, str } from './phone.svelte';

  /**
   * Voice mode: the computer reads the text as a job (ADR 0037), so a locked
   * screen does not cancel a long text. Language and tempo as in the Workshop.
   */
  const SLOT = 'voice';
  const LANGUAGES = ['pl', 'en', 'de', 'fr', 'es', 'it', 'pt', 'nl', 'ru'] as const;
  const MAX_CHARS = 20_000;

  let voices = $state<RemoteVoice[]>([]);
  let voiceId = $state<number | null>(null);
  let text = $state('');
  /** "" = the model's own language; a cloned voice keeps the language of its sample. */
  let language = $state('');
  let speed = $state(readLocal<number>('lokalsi.mobile.voice.speed', 1));
  let model = $state('');
  let job = $state<JobView | null>(null);
  let url = $state<string | null>(null);
  let error = $state('');
  let note = $state('');
  let starting = $state(false);
  let watch: Watch | undefined;

  let working = $derived(starting || (job !== null && !finished(job)));

  $effect(() => writeLocal('lokalsi.mobile.voice.speed', speed));

  // "Użyj ponownie" in the Gallery.
  $effect(() => {
    const p = phone.reuse?.kind === 'tts' ? phone.take('tts') : null;
    if (!p) return;
    text = str(p, 'text');
    voiceId = num(p, 'voice_id');
    language = voiceId === null ? str(p, 'language') : '';
    const saved = (p['settings'] ?? {}) as Record<string, unknown>;
    speed = num(saved, 'speed') ?? 1;
    if (str(p, 'model')) model = str(p, 'model');
  });

  onMount(() => {
    void api.voices().then((v) => (voices = v)).catch(() => undefined);
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
    const file = pickFile(j, 'audio/wav');
    if (!file) return;
    try {
      const next = fileUrl(await download(j, file.index));
      if (url) URL.revokeObjectURL(url);
      url = next;
    } catch (e) {
      error = describe(e);
    }
  }

  async function speak() {
    error = '';
    note = '';
    starting = true;
    try {
      const started = await api.startSpeech({
        input: text.trim(),
        voice_id: voiceId,
        language: voiceId === null ? language || null : null,
        model: model || null,
        speed: speed === 1 ? null : speed,
      });
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
    const file = job && pickFile(job, 'audio/wav');
    if (!job || !file) return;
    note = '';
    error = '';
    try {
      if (await api.saveFile(job.job_id, file.index, fileName(t('mobile.gallery.voiceFile'), job, file))) note = t('jobs.saved');
    } catch (e) {
      error = describe(e);
    }
  }
</script>

<div class="panel">
  {#if error}<p class="note note-error" role="alert">{error}</p>{/if}
  <ModelPicker mode="tts" bind:value={model} />
  <label class="field">
    <span class="small muted">{t('workshop.tts.voice')}</span>
    <select value={voiceId ?? ''} onchange={(e) => (voiceId = e.currentTarget.value ? Number(e.currentTarget.value) : null)}>
      <option value="">{t('workshop.tts.modelVoice')}</option>
      {#each voices as v (v.id)}<option value={v.id}>{v.name}</option>{/each}
    </select>
  </label>
  {#if voiceId === null}
    <label class="field">
      <span class="small muted">{t('mobile.voice.language')}</span>
      <select bind:value={language}>
        <option value="">{t('mobile.voice.languageAuto')}</option>
        {#each LANGUAGES as l (l)}<option value={l}>{t(`workshop.stt.languages.${l}`)}</option>{/each}
      </select>
    </label>
  {/if}
  <label class="field">
    <span class="small muted">{t('mobile.voice.speed')}: {speed.toFixed(1)}×</span>
    <input type="range" min="0.5" max="2" step="0.1" bind:value={speed} />
  </label>
  <textarea rows="4" bind:value={text} maxlength={MAX_CHARS} placeholder={t('workshop.tts.placeholder')} aria-label={t('workshop.tts.placeholder')}></textarea>
  <button type="button" class="btn btn-primary" onclick={speak} disabled={working || !text.trim()}>{t('workshop.tts.speak')}</button>
  {#if job}<JobProgress {job} onCancel={cancel} />{/if}
  {#if url && job?.status === 'done'}
    <audio controls src={url}></audio>
    <button type="button" class="btn" onclick={save}><DownloadSimple size={18} aria-hidden="true" />{t('mobile.save')}</button>
    {#if note}<p class="muted small" role="status">{note}</p>{/if}
  {/if}
</div>

<style>
  .panel {
    display: grid;
    gap: var(--space-3);
  }

  .field {
    display: grid;
    gap: var(--space-1);
  }

  textarea,
  select,
  input[type='range'],
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
