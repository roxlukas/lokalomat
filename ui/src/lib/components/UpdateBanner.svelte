<script lang="ts">
  import Download from '@lucide/svelte/icons/download';
  import X from '@lucide/svelte/icons/x';
  import { t } from '../i18n/index.svelte';
  import { updates } from '../updates.svelte';
  import ProgressBar from './ProgressBar.svelte';

  /** A new version (ADR 0029): one line under the header until installed or put off. */
</script>

{#if updates.available && !updates.dismissed}
  <div class="banner" role="status">
    <Download size={16} aria-hidden="true" />
    <span class="text">{t('updates.available', { version: updates.available.version })}</span>
    {#if updates.installing}
      <ProgressBar value={updates.progress} label={t('updates.installing')} />
      <span class="muted">{t('updates.installing')}</span>
    {:else}
      <button type="button" class="btn btn-primary small" onclick={() => updates.install()}>
        {updates.available.manual ? t('updates.download') : t('updates.install')}
      </button>
      <button type="button" class="btn small icon" aria-label={t('updates.later')} title={t('updates.later')} onclick={() => (updates.dismissed = true)}>
        <X size={14} aria-hidden="true" />
      </button>
    {/if}
    {#if updates.error}<span class="error" role="alert">{updates.error}</span>{/if}
  </div>
{/if}

<style>
  .banner {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: center;
    padding: var(--space-2) var(--space-4);
    border-bottom: 1px solid var(--border);
    background: var(--info-bg);
    color: var(--text);
    font-size: var(--text-s);
  }

  .text {
    flex: 1;
    min-width: 200px;
  }

  .muted {
    color: var(--text-muted);
  }

  .error {
    color: var(--error);
  }

  .btn.small {
    min-height: 28px;
    padding: 0 var(--space-2);
    font-size: var(--text-s);
  }
</style>
