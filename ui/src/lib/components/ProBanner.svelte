<script lang="ts">
  import Sparkles from '@lucide/svelte/icons/sparkles';
  import X from '@lucide/svelte/icons/x';
  import { t } from '../i18n/index.svelte';
  import { edition } from '../edition.svelte';
  import { PRO_URL, openExternal } from '../external';
  import { closeBanner, isBannerClosed } from '../proBanner';

  /** A small offer for Lokalomat Pro (ADR 0035): free edition only, can be closed. */
  let closed = $state(isBannerClosed());

  function close() {
    closeBanner();
    closed = true;
  }
</script>

{#if edition.loaded && !edition.isPro && !closed}
  <div class="banner" role="region" aria-label={t('pro.banner.action')}>
    <Sparkles size={16} aria-hidden="true" />
    <span class="text">{t('pro.banner.text')}</span>
    <a class="btn btn-primary small" href={PRO_URL} target="_blank" rel="noopener" onclick={(e) => openExternal(e, PRO_URL)}>
      {t('pro.banner.action')}
    </a>
    <button type="button" class="btn small icon" aria-label={t('pro.banner.close')} title={t('pro.banner.close')} onclick={close}>
      <X size={14} aria-hidden="true" />
    </button>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: center;
    padding: var(--space-1) var(--space-4);
    border-bottom: 1px solid var(--border);
    background: var(--accent-soft);
    color: var(--text);
    font-size: var(--text-s);
  }

  .text {
    flex: 1;
    min-width: 200px;
  }

  .btn.small {
    display: inline-flex;
    align-items: center;
    min-height: 28px;
    padding: 0 var(--space-2);
    font-size: var(--text-s);
    text-decoration: none;
  }
</style>
