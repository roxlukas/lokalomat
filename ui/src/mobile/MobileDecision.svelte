<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import Scales from 'phosphor-svelte/lib/ScalesIcon';
  import { getLocale, t } from '../lib/i18n/index.svelte';
  import { readLocal, writeLocal } from '../lib/storage';
  import { canAsk, cleanQuestions, labelsOf, MAX_STATE_CHARS, NO, starterQuestions, YES } from '../lib/decisions';
  import DecisionQuestions from '../lib/components/DecisionQuestions.svelte';
  import ProbabilityBar from '../lib/components/ProbabilityBar.svelte';
  import { api, describe, finished, type DecisionQuestion, type JobView } from './api';
  import { rememberJob, rememberedJob, watchJob, type Watch } from './jobs';
  import JobProgress from './JobProgress.svelte';
  import ModelPicker from './ModelPicker.svelte';

  /**
   * Decisions (ADR 0024): typed questions about a text, answered on the
   * computer with the probability of every answer. Shown only while the
   * computer is in Expert mode (ADR 0027).
   */
  const SLOT = 'decision';

  let text = $state(readLocal<string>('lokalsi.mobile.decision.text', ''));
  let questions = $state<DecisionQuestion[]>(
    readLocal<DecisionQuestion[] | null>('lokalsi.mobile.decision.questions', null) ?? starterQuestions(getLocale() === 'en' ? 'en' : 'pl'),
  );
  /** The questions of the shown result (the form may change meanwhile). */
  let asked = $state<DecisionQuestion[]>(readLocal<DecisionQuestion[]>('lokalsi.mobile.decision.asked', []));
  let model = $state('');
  let job = $state<JobView | null>(null);
  let error = $state('');
  let starting = $state(false);
  let watch: Watch | undefined;

  let working = $derived(starting || (job !== null && !finished(job)));
  let oneDecimal = $derived(new Intl.NumberFormat(getLocale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 }));

  $effect(() => writeLocal('lokalsi.mobile.decision.text', text));
  $effect(() => writeLocal('lokalsi.mobile.decision.questions', $state.snapshot(questions)));

  onMount(() => {
    const id = rememberedJob(SLOT);
    if (id !== null) follow(id);
  });

  onDestroy(() => watch?.stop());

  function follow(id: number) {
    watch?.stop();
    watch = watchJob(
      id,
      (j) => (job = j),
      (e) => {
        rememberJob(SLOT, null);
        job = null;
        if ((e as { code?: string })?.code !== 'not_found') error = describe(e);
      },
    );
  }

  async function ask() {
    error = '';
    starting = true;
    const qs = cleanQuestions(questions);
    try {
      const started = await api.startDecision(text.trim(), qs, model || null);
      asked = qs;
      writeLocal('lokalsi.mobile.decision.asked', qs);
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

  /** Yes/no are translated; options and levels are shown as typed. */
  function shown(label: string): string {
    if (label === YES) return t('decisions.yes');
    if (label === NO) return t('decisions.no');
    return label;
  }
</script>

<div class="panel">
  {#if error}<p class="note note-error" role="alert">{error}</p>{/if}
  <ModelPicker mode="decision" bind:value={model} />
  <textarea rows="4" bind:value={text} maxlength={MAX_STATE_CHARS} placeholder={t('workshop.decision.placeholder')} aria-label={t('workshop.decision.placeholder')}></textarea>
  <DecisionQuestions bind:questions />
  <p class="muted small">{t('workshop.decision.note')}</p>
  <button type="button" class="btn btn-primary" onclick={ask} disabled={working || !canAsk(text, questions)}>
    <Scales size={18} aria-hidden="true" />{t('workshop.decision.ask')}
  </button>
  {#if job}
    <JobProgress {job} onCancel={cancel} />
    {#if job.status === 'loading_model'}<p class="muted small">{t('decisions.loading')}</p>{/if}
  {/if}
  {#if job?.status === 'done'}
    <section class="result" aria-label={t('decisions.result')}>
      <h2>{t('decisions.result')}</h2>
      {#each asked as q (q.id)}
        {@const a = job.answers.find((x) => x.id === q.id)}
        {#if a}
          <div class="question">
            <h3>{q.instructions}</h3>
            <p class="answer">
              {shown(a.label)}
              <span class="muted small">· {t('decisions.confidence', { percent: Math.round(a.confidence * 100) })}</span>
              {#if q.type === 'score' && a.score != null}
                <span class="muted small">· {t('decisions.score', { value: oneDecimal.format(a.score + 1), max: labelsOf(q).length })}</span>
              {/if}
            </p>
            <div class="bars">
              {#each a.probabilities as pr (pr.label)}
                <ProbabilityBar label={shown(pr.label)} p={pr.p} top={pr.label === a.label} />
              {/each}
            </div>
          </div>
        {/if}
      {/each}
      <p class="muted small">{job.model}</p>
    </section>
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

  .result {
    display: grid;
    gap: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-l);
    background: var(--surface);
  }

  h2 {
    font-size: var(--text-m);
  }

  h3 {
    font-size: var(--text-s);
    font-weight: 600;
    color: var(--text-muted);
    overflow-wrap: anywhere;
  }

  .question {
    display: grid;
    gap: var(--space-2);
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
  }

  .answer {
    font-size: var(--text-m);
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .answer .muted {
    font-weight: 400;
  }

  .bars {
    display: grid;
    gap: 6px;
  }

  .small {
    font-size: var(--text-s);
  }

  .muted {
    color: var(--text-muted);
  }
</style>
