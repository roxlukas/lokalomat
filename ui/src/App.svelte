<script lang="ts">
  import { onMount } from 'svelte';
  import Header from './lib/components/Header.svelte';
  import Rail from './lib/components/Rail.svelte';
  import Inspector from './lib/components/Inspector.svelte';
  import CommandPalette from './lib/components/CommandPalette.svelte';
  import StatusBar from './lib/components/StatusBar.svelte';
  import UpdateBanner from './lib/components/UpdateBanner.svelte';
  import ProBanner from './lib/components/ProBanner.svelte';
  import { edition } from './lib/edition.svelte';
  import { updates } from './lib/updates.svelte';
  import { initJobs } from './lib/jobs.svelte';
  import ViewHost from './lib/components/ViewHost.svelte';
  import SlotHost from './lib/components/SlotHost.svelte';
  import { blocked, findView, runStartups } from './lib/edition/registry';
  import { initChat } from './lib/chat.svelte';
  import OnboardingWizard from './views/OnboardingWizard.svelte';
  import { initStores } from './lib/stores.svelte';
  import { t } from './lib/i18n/index.svelte';
  import { router } from './lib/router.svelte';
  import { settings } from './lib/settings.svelte';
  import { logToApp } from './lib/errors';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import EmptyState from './lib/components/EmptyState.svelte';
  import { errorMessage } from './lib/ipc';

  let paletteOpen = $state(false);

  onMount(() => {
    settings.load().catch((e) => logToApp('error', `cannot load settings: ${errorMessage(e)}`));
    initStores().catch((e) => logToApp('error', `cannot load application state: ${errorMessage(e)}`));
    initChat().catch((e) => logToApp('error', `cannot load chat state: ${errorMessage(e)}`));
    initJobs().catch((e) => logToApp('error', `cannot load jobs: ${errorMessage(e)}`));
    void runStartups((e) => logToApp('error', `edition start-up failed: ${errorMessage(e)}`));
    edition.load().catch((e) => logToApp('warn', `cannot read the edition: ${errorMessage(e)}`));
    updates.init().catch((e) => logToApp('warn', `cannot listen for updates: ${errorMessage(e)}`));
  });

  const wizardOpen = $derived(settings.loaded && !settings.value.onboarded);
  // The page behind a dialog must not take focus.
  const covered = $derived(wizardOpen || blocked());

  function onKeydown(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      paletteOpen = !paletteOpen;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<!-- While the first-run wizard covers the screen, the page behind it must not take focus. -->
<a class="skip" href="#main" inert={covered} onclick={(e) => (e.preventDefault(), document.getElementById('main')?.focus())}>
  {t('app.skipToContent')}
</a>

<div class="shell" inert={covered}>
  <Header onOpenPalette={() => (paletteOpen = true)} />
  <UpdateBanner />
  <ProBanner />
  <div class="body">
    <Rail />
    <main id="main" class="main" tabindex="-1">
      <svelte:boundary onerror={(e) => logToApp('error', `view crashed: ${errorMessage(e)}`)}>
      {#key router.current}
        <ViewHost load={findView(router.current)?.load} />
      {/key}
        {#snippet failed(_error, reset)}
          <EmptyState icon={TriangleAlert} title={t('app.crash.title')} body={t('app.crash.body')}>
            <button type="button" class="btn btn-primary" onclick={reset}>{t('app.crash.retry')}</button>
          </EmptyState>
        {/snippet}
      </svelte:boundary>
    </main>
    <Inspector />
  </div>
  <StatusBar />
</div>

<CommandPalette bind:open={paletteOpen} />

{#if settings.loaded && !settings.value.onboarded}
  <OnboardingWizard />
{/if}

<SlotHost name="overlay" />

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .body {
    position: relative;
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .main {
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }

  .main:focus {
    outline: none;
  }

  .skip {
    position: absolute;
    left: var(--space-2);
    top: -40px;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-s);
    background: var(--accent);
    color: var(--accent-contrast);
    z-index: 100;
  }

  .skip:focus {
    top: var(--space-2);
  }
</style>
