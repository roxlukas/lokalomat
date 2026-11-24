<script lang="ts">
  import Stop from 'phosphor-svelte/lib/StopIcon';
  import { t } from '../lib/i18n/index.svelte';
  import { describeJob, finished, type JobView } from './api';

  /** Where a job on the computer is: its stage, progress and a stop button. */
  let { job, onCancel }: { job: JobView; onCancel?: () => void } = $props();

  let pct = $derived(job.progress == null ? null : Math.round(Math.max(0, Math.min(1, job.progress)) * 100));
  let hint = $derived(
    job.status === 'preparing' ? t('mobile.jobs.preparing') : job.status === 'queued' ? t('mobile.jobs.queued') : '',
  );
</script>

{#if job.status === 'failed' || job.status === 'cancelled'}
  <p class="note note-error" role="alert">{describeJob(job)}</p>
{:else if !finished(job)}
  <div class="progress" role="status">
    <div class="row">
      <span class="small">{t(`jobs.status.${job.status}`)}{pct != null && job.status === 'running' ? ` · ${pct}%` : ''}</span>
      {#if onCancel}
        <button type="button" class="btn small" onclick={onCancel}>
          <Stop size={14} aria-hidden="true" />{t('mobile.jobs.stop')}
        </button>
      {/if}
    </div>
    {#if job.status === 'running' && pct != null}
      <progress max="100" value={pct} aria-label={t(`jobs.status.${job.status}`)}></progress>
    {:else}
      <progress aria-label={t(`jobs.status.${job.status}`)}></progress>
    {/if}
    {#if hint}<p class="muted small">{hint}</p>{/if}
  </div>
{/if}

<style>
  .progress {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    background: var(--surface);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  progress {
    width: 100%;
    accent-color: var(--accent);
  }

  .btn.small {
    min-height: 32px;
    padding: 0 var(--space-3);
    font-size: var(--text-s);
  }

  .small {
    font-size: var(--text-s);
  }

  .muted {
    color: var(--text-muted);
  }
</style>
