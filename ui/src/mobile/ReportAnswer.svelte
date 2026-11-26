<script lang="ts">
  import Flag from 'phosphor-svelte/lib/FlagIcon';
  import { t } from '../lib/i18n/index.svelte';
  import { answerReportMailto } from '../lib/report';
  import { inApp } from './api';

  /**
   * "Zgłoś" under an AI answer (ADR 0040, Google Play's AI-generated content
   * policy): opens the mail app with the answer filled in. Nothing is sent
   * until the user presses Send there.
   */
  let { answer, model = '', where }: { answer: string; model?: string; where: string } = $props();

  async function report() {
    const version = inApp ? await import('@tauri-apps/api/app').then((m) => m.getVersion()).catch(() => '') : '';
    const { url } = answerReportMailto({ answer, model: model || undefined, where, version: version || undefined }, t('mobile.report.reason'));
    if (!inApp) {
      window.location.href = url;
      return;
    }
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl(url).catch(() => undefined);
  }
</script>

<button type="button" class="report" onclick={() => void report()} title={t('mobile.report.hint')}>
  <Flag size={14} aria-hidden="true" />{t('mobile.report.label')}
</button>

<style>
  .report {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    justify-self: start;
    min-height: 32px;
    padding: 0 var(--space-2);
    border: 0;
    background: none;
    color: var(--text-faint);
    font-size: var(--text-xs);
  }

  .report:hover {
    color: var(--error);
  }
</style>
