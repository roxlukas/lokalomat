<script lang="ts">
  import { t } from '../i18n/index.svelte';
  import { router } from '../router.svelte';
  import { models } from '../stores.svelte';
  import type { Modality } from '../ipc';
  import type { engineStatus } from '../workshop.svelte';

  /** The engine and model notes of a mode that needs a model (everything but chat). */
  let {
    mode,
    status,
    optionCount,
  }: { mode: string; status: ReturnType<typeof engineStatus>; optionCount: number } = $props();
</script>

{#if status.unavailable}
  <p class="note note-warn setup">{t('workshop.engineUnavailable')}</p>
{:else if status.install}
  <p class="note note-info setup" role="status">
    {t('workshop.preparingEngine', { percent: status.install[1] ? Math.round((status.install[0] / status.install[1]) * 100) : 0 })}
  </p>
{/if}
{#if optionCount === 0}
  <p class="note note-warn setup">
    {models.list.some((m) => m.modality === mode && m.variants.some((v) => v.state === 'partial'))
      ? t('workshop.partialModel')
      : t(`workshop.${mode}.needModel`)}
    <button type="button" class="link" onclick={() => router.goModels(mode as Modality)}>{t('chat.goModels')}</button>
  </p>
{/if}
