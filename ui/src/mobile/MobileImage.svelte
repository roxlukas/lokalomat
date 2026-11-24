<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import DownloadSimple from 'phosphor-svelte/lib/DownloadSimpleIcon';
  import { t } from '../lib/i18n/index.svelte';
  import { readLocal, writeLocal } from '../lib/storage';
  import { api, describe, fileUrl, finished, type JobView } from './api';
  import { download, fileName, rememberJob, rememberedJob, watchJob, type Watch } from './jobs';
  import JobProgress from './JobProgress.svelte';
  import ModelPicker from './ModelPicker.svelte';
  import { num, phone, str, wholeOrNull } from './phone.svelte';

  /**
   * Image mode: the computer draws the pictures as a job (ADR 0027), so a
   * locked screen does not stop it. Count, and in Expert mode the negative
   * prompt, seed, steps and CFG, as in the Workshop (ADR 0037).
   */
  const SLOT = 'image';
  const ASPECTS = ['1:1', '3:4', '16:9'];
  const COUNTS = [1, 2, 4];

  let prompt = $state('');
  let aspect = $state('1:1');
  let count = $state(readLocal<number>('lokalsi.mobile.image.count', 1));
  let negative = $state('');
  let seed = $state<number | null>(null);
  let steps = $state<number | null>(null);
  let cfg = $state<number | null>(null);
  let more = $state(false);
  let model = $state('');
  let job = $state<JobView | null>(null);
  let urls = $state<string[]>([]);
  let error = $state('');
  let note = $state('');
  let starting = $state(false);
  let watch: Watch | undefined;

  let working = $derived(starting || (job !== null && !finished(job)));

  $effect(() => writeLocal('lokalsi.mobile.image.count', count));

  // "Użyj ponownie" in the Gallery: the same settings, the same seed.
  $effect(() => {
    const p = phone.reuse?.kind === 'image' ? phone.take('image') : null;
    if (!p) return;
    prompt = str(p, 'prompt');
    negative = str(p, 'negative_prompt');
    const a = str(p, 'aspect');
    if (ASPECTS.includes(a)) aspect = a;
    const c = num(p, 'count');
    if (c !== null && COUNTS.includes(c)) count = c;
    seed = num(p, 'seed');
    steps = num(p, 'steps');
    cfg = num(p, 'cfg_scale');
    if (str(p, 'model')) model = str(p, 'model');
    if (phone.expert && (negative || steps !== null || cfg !== null)) more = true;
  });

  onMount(() => {
    const id = rememberedJob(SLOT);
    if (id !== null) follow(id);
  });

  onDestroy(() => {
    watch?.stop();
    clearImages();
  });

  function clearImages() {
    urls.forEach((u) => URL.revokeObjectURL(u));
    urls = [];
  }

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
    const files = j.files.filter((f) => f.content_type === 'image/png');
    try {
      const next: string[] = [];
      for (const f of files) next.push(fileUrl(await download(j, f.index)));
      clearImages();
      urls = next;
    } catch (e) {
      error = describe(e);
    }
  }

  async function generate() {
    error = '';
    note = '';
    starting = true;
    try {
      const expert = phone.expert;
      const started = await api.startImage({
        prompt: prompt.trim(),
        aspect,
        model: model || null,
        count,
        // Expert settings, as on the computer; Basic leaves them to the model.
        negative_prompt: expert ? negative.trim() : '',
        seed: expert ? wholeOrNull(seed, 0, 4_294_967_295) : null,
        steps: expert ? wholeOrNull(steps, 1, 150) : null,
        cfg_scale: expert && cfg !== null && Number.isFinite(cfg) ? Math.min(30, Math.max(0, cfg)) : null,
      });
      clearImages();
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

  async function save(position: number) {
    const file = job?.files.filter((f) => f.content_type === 'image/png')[position];
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
  {#if phone.expert}
    <textarea rows="2" bind:value={negative} placeholder={t('mobile.image.negative')} aria-label={t('mobile.image.negative')}></textarea>
  {/if}
  <div class="chips" role="radiogroup" aria-label={t('workshop.image.aspect')}>
    {#each ASPECTS as a (a)}
      <button type="button" role="radio" aria-checked={aspect === a} class="chip" class:active={aspect === a} onclick={() => (aspect = a)}>{a}</button>
    {/each}
  </div>
  <div class="chips" role="radiogroup" aria-label={t('mobile.image.count')}>
    {#each COUNTS as c (c)}
      <button type="button" role="radio" aria-checked={count === c} class="chip" class:active={count === c} onclick={() => (count = c)}>×{c}</button>
    {/each}
  </div>
  {#if phone.expert}
    <button type="button" class="link-btn small" aria-expanded={more} onclick={() => (more = !more)}>{t('mobile.more')}</button>
    {#if more}
      <div class="fields">
        <label class="field">
          <span class="small muted">{t('mobile.seed')}</span>
          <input type="number" min="0" inputmode="numeric" bind:value={seed} placeholder={t('mobile.random')} />
        </label>
        <label class="field">
          <span class="small muted">{t('mobile.image.steps')}</span>
          <input type="number" min="1" max="150" inputmode="numeric" bind:value={steps} placeholder={t('mobile.auto')} />
        </label>
        <label class="field">
          <span class="small muted">{t('mobile.image.cfg')}</span>
          <input type="number" min="0" max="30" step="0.5" inputmode="decimal" bind:value={cfg} placeholder={t('mobile.auto')} />
        </label>
      </div>
    {/if}
  {/if}
  <button type="button" class="btn btn-primary" onclick={generate} disabled={working || !prompt.trim()}>{t('workshop.image.generate')}</button>
  {#if job}<JobProgress {job} onCancel={cancel} />{/if}
  {#if urls.length > 0 && job?.status === 'done'}
    {#each urls as u, i (u)}
      <img src={u} alt={prompt || t('jobs.imageOf')} />
      <button type="button" class="btn" onclick={() => save(i)}><DownloadSimple size={18} aria-hidden="true" />{t('mobile.save')}</button>
    {/each}
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

  .fields {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: var(--space-2);
  }

  .field {
    display: grid;
    gap: 4px;
  }

  .field input {
    width: 100%;
    min-width: 0;
  }

  .link-btn {
    justify-self: start;
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    text-decoration: underline;
  }

  .small {
    font-size: var(--text-s);
  }

  .muted {
    color: var(--text-muted);
  }
</style>
