<script lang="ts">
  import Scale from '@lucide/svelte/icons/scale';
  import Copy from '@lucide/svelte/icons/copy';
  import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import { getLocale, t } from '../i18n/index.svelte';
  import type { DecisionQuestion, Job } from '../ipc';
  import { jobs } from '../jobs.svelte';
  import { jobDecision, labelsOf, YES, NO } from '../decisions';
  import JobStatus from './JobStatus.svelte';
  import ProbabilityBar from './ProbabilityBar.svelte';

  /** A decision result (Workshop "Decyzje"): every question with its answer and probabilities. */
  let { job, onReuse }: { job: Job; onReuse?: (state: string, questions: DecisionQuestion[]) => void } = $props();

  let note = $state('');
  let confirmDelete = $state(false);
  let expanded = $state(false);
  let decision = $derived(jobDecision(job));
  let longText = $derived(job.text.length > 220);
  /** One decimal in the UI language (`2,5` in Polish). */
  let oneDecimal = $derived(new Intl.NumberFormat(getLocale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 }));

  /** Shown text of an answer label: yes/no are translated. */
  function shown(label: string): string {
    if (label === YES) return t('decisions.yes');
    if (label === NO) return t('decisions.no');
    return label;
  }

  async function copyJson() {
    note = '';
    const data = { state: job.text, questions: decision.questions, answers: decision.answers };
    try {
      await navigator.clipboard.writeText(JSON.stringify(data, null, 2));
      note = t('decisions.copied');
    } catch {
      note = t('decisions.copyFailed');
    }
  }
</script>

<article class="card" aria-label={t('decisions.result')}>
  <header>
    <Scale size={18} aria-hidden="true" />
    <div class="title">
      <h3>{t('decisions.result')}</h3>
      <span class="muted small">{job.model_name}{job.duration_ms ? ` · ${oneDecimal.format(job.duration_ms / 1000)} s` : ''}</span>
    </div>
  </header>

  <p class="text" class:clamped={longText && !expanded}>{job.text}</p>
  {#if longText}
    <button type="button" class="link small" onclick={() => (expanded = !expanded)}>{expanded ? t('jobs.less') : t('jobs.more')}</button>
  {/if}

  <JobStatus {job} onCancel={(id) => jobs.cancel(id)} />

  {#if job.status === 'done'}
    {#each decision.questions as q (q.id)}
      {@const a = decision.answers.find((x) => x.id === q.id)}
      <section class="question">
        <h4>{q.instructions}</h4>
        {#if a}
          <p class="answer">
            {shown(a.label)}
            <span class="muted small" title={t('decisions.confidenceHint')}>· {t('decisions.confidence', { percent: Math.round(a.confidence * 100) })}</span>
            {#if q.type === 'score' && a.score != null}
              <span class="muted small" title={t('decisions.scoreHint')}>· {t('decisions.score', { value: oneDecimal.format(a.score + 1), max: labelsOf(q).length })}</span>
            {/if}
          </p>
          <div class="bars">
            {#each a.probabilities as pr (pr.label)}
              <ProbabilityBar label={shown(pr.label)} p={pr.p} top={pr.label === a.label} />
            {/each}
          </div>
        {/if}
      </section>
    {/each}
    <div class="actions">
      {#if onReuse}
        <button type="button" class="btn small" onclick={() => onReuse(job.text, decision.questions)}>
          <RotateCcw size={14} aria-hidden="true" />{t('decisions.reuse')}
        </button>
      {/if}
      <button type="button" class="btn small" onclick={copyJson} title={t('decisions.copyHint')}>
        <Copy size={14} aria-hidden="true" />{t('decisions.copy')}
      </button>
      {#if confirmDelete}
        <button type="button" class="btn small danger" onclick={() => jobs.remove([job.id])}>{t('jobs.confirmDelete')}</button>
      {:else}
        <button type="button" class="btn small" onclick={() => (confirmDelete = true)} aria-label={t('jobs.delete')}><Trash2 size={14} aria-hidden="true" /></button>
      {/if}
    </div>
    {#if note}<p class="muted small" role="status">{note}</p>{/if}
  {:else if job.status === 'preparing'}
    <p class="muted small">{t('decisions.preparing')}</p>
  {:else if job.status === 'loading_model'}
    <p class="muted small">{t('decisions.loading')}</p>
  {/if}
</article>

<style>
  .card {
    display: grid;
    gap: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-l);
    background: var(--surface);
  }

  header {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }

  h3 {
    font-size: var(--text-m);
  }

  h4 {
    font-size: var(--text-s);
    font-weight: 600;
    color: var(--text-muted);
    overflow-wrap: anywhere;
  }

  .title {
    display: grid;
  }

  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .text.clamped {
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
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

  .link {
    justify-self: start;
    padding: 0;
    border: 0;
    background: none;
    color: var(--focus);
    text-decoration: underline;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .btn.small {
    min-height: 28px;
    padding: 0 var(--space-2);
    font-size: var(--text-s);
  }

  .small {
    font-size: var(--text-s);
  }
</style>
