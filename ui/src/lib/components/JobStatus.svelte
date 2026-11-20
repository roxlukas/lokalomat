<script lang="ts">
  import Square from '@lucide/svelte/icons/square';
  import { t } from '../i18n/index.svelte';
  import type { Job } from '../ipc';
  import { describeJobError } from '../jobs.svelte';
  import { jobFinished } from '../media';
  import ProgressBar from './ProgressBar.svelte';

  let { job, onCancel }: { job: Job; onCancel: (id: number) => void } = $props();
  let running = $derived(!jobFinished(job));
  let label = $derived(t(`jobs.status.${job.status}`));
</script>

{#if running}
  <div class="status" role="status">
    <div class="line">
      <span class="label">{label}</span>
      {#if job.status === 'running' && job.progress != null}<span class="pct">{Math.round(job.progress * 100)}%</span>{/if}
      <button type="button" class="btn small" onclick={() => onCancel(job.id)}>
        <Square size={12} aria-hidden="true" />{t('common.cancel')}
      </button>
    </div>
    <ProgressBar value={job.status === 'running' ? job.progress : null} {label} />
  </div>
{:else if job.status === 'failed'}
  <p class="note note-error" title={job.error ?? ''}>{describeJobError(job.error)}</p>
{:else if job.status === 'cancelled'}
  <p class="muted small">{t('jobs.status.cancelled')}</p>
{/if}

<style>
  .status {
    display: grid;
    gap: var(--space-2);
  }

  .line {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-s);
  }

  .label {
    color: var(--text-muted);
  }

  .pct {
    font-variant-numeric: tabular-nums;
  }

  .line .btn {
    margin-left: auto;
  }

  .btn.small {
    min-height: 26px;
    padding: 0 var(--space-2);
    font-size: var(--text-s);
  }
</style>
