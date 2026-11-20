<script lang="ts">
  import Lock from '@lucide/svelte/icons/lock';
  import { t } from '../i18n/index.svelte';
  import type { License } from '../ipc';

  let { license }: { license: License } = $props();
</script>

<span class="badges">
  <span class="lic lic-{license.commercial}" title={license.name || t('license.unknown')}>
    {t(`license.commercial.${license.commercial}`)}
  </span>
  {#if license.gated}
    <span class="lic lic-gated" title={t('license.gated')}>
      <Lock size={12} aria-hidden="true" /><span class="sr-only">{t('license.gated')}</span>
    </span>
  {/if}
  {#if !license.eu_allowed}
    <span class="lic lic-no">{t('license.euBlocked')}</span>
  {/if}
</span>

<style>
  .badges {
    display: inline-flex;
    gap: 4px;
  }

  .lic {
    display: inline-flex;
    align-items: center;
    padding: 1px 8px;
    border-radius: var(--radius-s);
    border: 1px solid transparent;
    font-size: var(--text-xs);
    white-space: nowrap;
  }

  /* PRD V4: green / amber "conditions" / grey "non-commercial" */
  .lic-yes {
    background: var(--ok-bg);
    color: var(--ok);
  }

  .lic-conditions {
    background: var(--warn-bg);
    color: var(--warn);
  }

  .lic-no {
    background: var(--surface-2);
    color: var(--text-muted);
    border-color: var(--border);
  }

  .lic-gated {
    padding: 1px 5px;
    background: var(--surface-2);
    color: var(--text-muted);
  }
</style>
