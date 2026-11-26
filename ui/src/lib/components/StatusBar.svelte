<script lang="ts">
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import { t } from '../i18n/index.svelte';
  import { openExternal } from '../external';

  const SITE = 'https://aitransformation.pl';
</script>

<footer class="status">
  <a class="credit" href={SITE} target="_blank" rel="noopener" onclick={(e) => openExternal(e, SITE)} aria-label={t('footer.linkLabel')}>
    <span class="site">{t('footer.site')}</span>
    <ExternalLink size={11} aria-hidden="true" />
  </a>
</footer>

<style>
  .status {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    height: 24px;
    padding: 0 var(--space-3);
    border-top: 1px solid var(--border);
    background: var(--surface);
    font-size: var(--text-xs);
  }

  .credit {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text-muted);
    text-decoration: none;
  }

  .credit:hover {
    color: var(--text);
  }

  .credit:hover .site {
    text-decoration: underline;
  }

</style>
