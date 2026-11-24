<script lang="ts">
  import Mail from '@lucide/svelte/icons/mail';
  import { t } from '../../lib/i18n/index.svelte';
  import { edition } from '../../lib/edition.svelte';
  import { openExternal } from '../../lib/external';
  import { reportMailto, REPORT_EMAIL } from '../../lib/report';
  import type { AppInfo } from '../../lib/ipc';

  let { info }: { info: AppInfo | null } = $props();

  let description = $state('');
  let reply = $state('');
  let attachSystem = $state(true);
  let truncated = $state(false);

  const link = $derived(
    reportMailto({
      description,
      reply,
      edition: edition.isPro ? 'pro' : 'free',
      version: info?.version ?? '',
      system: info ? `${info.os}/${info.arch}` : undefined,
      attachSystem,
    }),
  );

  function send(e: MouseEvent) {
    truncated = link.truncated;
    if (!description.trim()) {
      e.preventDefault();
      return;
    }
    void openExternal(e, link.url);
  }
</script>

<section aria-labelledby="s-report">
  <h2 id="s-report">{t('report.title')}</h2>
  <p class="muted">{t('report.intro')}</p>

  <div class="field">
    <label for="report-text">{t('report.what')}</label>
    <textarea id="report-text" rows="5" bind:value={description} placeholder={t('report.placeholder')}></textarea>
  </div>
  <div class="field">
    <label for="report-reply">{t('report.reply')}</label>
    <input id="report-reply" type="email" bind:value={reply} autocomplete="email" />
  </div>
  <label class="check">
    <input type="checkbox" bind:checked={attachSystem} />
    {t('report.attach')}
  </label>

  <p>
    <a
      class="btn btn-primary"
      class:disabled={!description.trim()}
      aria-disabled={!description.trim()}
      href={link.url}
      onclick={send}
    >
      <Mail size={16} aria-hidden="true" />{t('report.send')}
    </a>
  </p>
  <p class="muted small" role="status">
    {truncated ? t('report.truncated') : t('report.note', { email: REPORT_EMAIL })}
  </p>
</section>

<style>
  section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-l);
    background: var(--surface);
  }

  h2 {
    font-size: var(--text-l);
  }

  .muted {
    color: var(--text-muted);
  }

  .small {
    font-size: var(--text-s);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    border: 0;
  }

  .field > label {
    margin-bottom: var(--space-1);
    font-weight: 600;
    font-size: var(--text-s);
  }

  .check {
    display: inline-flex;
    gap: var(--space-2);
    align-items: center;
  }

  p {
    margin: 0;
  }

  a.disabled {
    opacity: 0.5;
    pointer-events: none;
  }
</style>
