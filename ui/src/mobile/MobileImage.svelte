<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import Download from '@lucide/svelte/icons/download';
  import { t } from '../lib/i18n/index.svelte';
  import { api, describe, fileUrl, finished, type JobView } from './api';
  import { download, fileName, pickFile, rememberJob, rememberedJob, watchJob, type Watch } from './jobs';
  import JobProgress from './JobProgress.svelte';
  import ModelPicker from './ModelPicker.svelte';

  /**
   * Image mode: the computer draws the picture as a job (ADR 0027), so a
   * locked screen does not stop it. The result can be saved on the phone.
   */
  const SLOT = 'image';
  const ASPECTS = ['1:1', '3:4', '16:9'];

  let prompt = $state('');
  let aspect = $state('1:1');
  let model = $state('');
  let job = $state<JobView | null>(null);
  let url = $state<string | null>(null);
  let error = $state('');
  let note = $state('');
  let starting = $state(false);
  let watch: Watch | undefined;

  let working = $derived(starting || (job !== null && !finished(job)));

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
        // A job the computer no longer knows: start fresh.
        rememberJob(SLOT, null);
        job = null;
        if ((e as { code?: string })?.code !== 'not_found') error = describe(e);
      },
    );
  }

  async function show(j: JobView) {
    const file = pickFile(j, 'image/png');
    if (!file) return;
    try {
      const next = fileUrl(await download(j, file.index));
      if (url) URL.revokeObjectURL(url);
      url = next;
    } catch (e) {
      error = describe(e);
    }
  }

  async function generate() {
    error = '';
    note = '';
    starting = true;
    try {
      const started = await api.startImage(prompt.trim(), aspect, model || null);
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
    const file = job && pickFile(job, 'image/png');
    if (!job || !file) return;
    note = '';
    error = '';
    try {
      if (await api.saveFile(job.job_id, file.index, fileName(t('mobile.image.file'), job, file))) note = t('jobs.saved');
    } catch (e) {
      error = describe(e);
    }
  }
</script>

<div class="panel">
  {#if error}<p class="note note-error" role="alert">{error}</p>{/if}
  <ModelPicker mode="image" bind:value={model} />
  <textarea rows="3" bind:value={prompt} placeholder={t('workshop.image.placeholder')} aria-label={t('workshop.image.placeholder')}></textarea>
  <div class="chips" role="radiogroup" aria-label={t('workshop.image.aspect')}>
    {#each ASPECTS as a (a)}
      <button type="button" role="radio" aria-checked={aspect === a} class="chip" class:active={aspect === a} onclick={() => (aspect = a)}>{a}</button>
    {/each}
  </div>
  <button type="button" class="btn btn-primary" onclick={generate} disabled={working || !prompt.trim()}>{t('workshop.image.generate')}</button>
  {#if job}<JobProgress {job} onCancel={cancel} />{/if}
  {#if url && job?.status === 'done'}
    <img src={url} alt={prompt || t('jobs.imageOf')} />
    <button type="button" class="btn" onclick={save}><Download size={18} aria-hidden="true" />{t('mobile.save')}</button>
    {#if note}<p class="muted small" role="status">{note}</p>{/if}
  {/if}
</div>

<style>
  .panel {
    display: grid;
    gap: var(--space-3);
  }

  textarea {
    width: 100%;
  }

  img {
    width: 100%;
    border-radius: var(--radius-m);
  }

  .chips {
    display: flex;
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

  .small {
    font-size: var(--text-s);
  }

  .muted {
    color: var(--text-muted);
  }
</style>
