<script lang="ts">
  import { onMount } from 'svelte';
  import Hammer from '@lucide/svelte/icons/hammer';
  import PanelLeft from '@lucide/svelte/icons/panel-left';
  import Square from '@lucide/svelte/icons/square';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import EmptyState from '../lib/components/EmptyState.svelte';
  import SessionDrawer from '../lib/components/SessionDrawer.svelte';
  import ViewHost from '../lib/components/ViewHost.svelte';
  import { registeredWorkshopModes } from '../lib/edition/registry';
  import { t } from '../lib/i18n/index.svelte';
  import { jobs } from '../lib/jobs.svelte';
  import { inTauri } from '../lib/ipc';
  import { isAudioPath } from '../lib/media';
  import { settings } from '../lib/settings.svelte';
  import { isSendKey, sendKeyAria, sendKeyHint } from '../lib/sendKey';
  import { readLocal, writeLocal } from '../lib/storage';
  import { MODE_KEY, workshop } from '../lib/workshop.svelte';
  import '../styles/workshop.css';

  /**
   * The Workshop frame (ADR 0035): mode tabs, output area and command bar. What
   * each mode shows and does comes from its panel (`views/workshop/*Panel.svelte`),
   * which the edition registered.
   */
  const DRAWER_KEY = 'lokalsi.sessions-open';

  let drawerOpen = $state(readLocal<boolean>(DRAWER_KEY, true));
  let streamEl: HTMLElement | undefined = $state();

  const modes = registeredWorkshopModes();
  let visibleModes = $derived(modes.filter((m) => settings.value.expert || !m.expert));
  let active = $derived(modes.find((m) => m.id === workshop.mode));
  let view = $derived(workshop.view);

  // Remember the last mode between visits.
  $effect(() => writeLocal(MODE_KEY, workshop.mode));
  // Basic mode hides the Expert modes: leave one when Expert is switched off.
  $effect(() => {
    if (!visibleModes.some((m) => m.id === workshop.mode)) workshop.mode = 'chat';
  });

  // "Odtwórz parametry w Warsztacie" (Gallery) and "same seed" on image cards:
  // the panel of the mode picks the parameters up when it opens.
  $effect(() => {
    const replay = jobs.replay;
    if (!replay) return;
    jobs.replay = null;
    const mode = replay.kind === 'image' || replay.kind === 'tts' || replay.kind === 'music' ? replay.kind : 'stt';
    workshop.mode = mode;
    workshop.reuse = { mode, params: replay.params };
  });

  // Keep the newest output in view, unless the user scrolled up.
  $effect(() => {
    void view?.scrollKey;
    if (workshop.stick && streamEl) streamEl.scrollTop = streamEl.scrollHeight;
  });

  function onScroll() {
    if (!streamEl) return;
    workshop.stick = streamEl.scrollHeight - streamEl.scrollTop - streamEl.clientHeight < 80;
  }

  // Audio files dropped on the window (Tauri gives real paths).
  onMount(() => {
    if (!inTauri) return;
    let unlisten: (() => void) | undefined;
    let stopped = false;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const p = event.payload;
        if (p.type === 'enter' || p.type === 'over') {
          workshop.dropActive = (view?.audioDrop?.show ?? false) || (p.type === 'enter' && p.paths.some(isAudioPath));
        } else if (p.type === 'leave') {
          workshop.dropActive = false;
        } else if (p.type === 'drop') {
          workshop.dropActive = false;
          const audio = p.paths.find(isAudioPath);
          if (!audio) return;
          if (view?.audioDrop?.handles) view.audioDrop.run(audio);
          else if (modes.some((m) => m.id === 'stt')) {
            // Another mode is open: switch to transcription, which takes the file.
            workshop.pendingAudio = audio;
            workshop.mode = 'stt';
          }
        }
      })
      .then((fn) => (stopped ? fn() : (unlisten = fn)));
    return () => {
      stopped = true;
      unlisten?.();
    };
  });

  function toggleDrawer() {
    drawerOpen = !drawerOpen;
    writeLocal(DRAWER_KEY, drawerOpen);
  }

  // Alt+1…7 switches the mode (PRD §9.3 V2); numbers follow the visible modes.
  function onKeydown(event: KeyboardEvent) {
    if (!event.altKey || event.ctrlKey || event.metaKey) return;
    const next = visibleModes[Number(event.key) - 1];
    if (next) {
      event.preventDefault();
      workshop.mode = next.id;
    }
  }

  async function send(e?: SubmitEvent) {
    e?.preventDefault();
    if (!view?.canSend) return;
    workshop.error = '';
    await view.send();
  }

  function onInputKey(e: KeyboardEvent) {
    // Ctrl/⌘+Enter sends (PRD V2); Settings can make plain Enter send (ADR 0036).
    if (isSendKey(e, settings.value.send_key)) {
      e.preventDefault();
      void send();
    }
  }

  const examples = $derived(modes.filter((m) => m.example && (settings.value.expert || !m.expert)));
</script>

<svelte:window onkeydown={onKeydown} />

<div class="workshop">
  {#if drawerOpen}<SessionDrawer />{/if}
  <div class="main">
    <div class="stream" bind:this={streamEl} onscroll={onScroll}>
      {#if view?.hasOutput && view.output}
        {@render view.output()}
      {:else}
        <EmptyState icon={Hammer} title={t('views.workshop.title')} body={t('views.workshop.body')}>
          <div class="examples">
            <p class="examples-title">{t('workshop.examplesTitle')}</p>
            <ul>
              {#each examples as example (example.id)}
                <li>
                  <button
                    type="button"
                    class="example"
                    onclick={() => ((workshop.mode = example.id), (workshop.prompt = t(`workshop.examples.${example.id}`)))}
                  >
                    <span class="example-mode">{t(`workshop.modes.${example.id}`)}</span>
                    {t(`workshop.examples.${example.id}`)}
                  </button>
                </li>
              {/each}
            </ul>
          </div>
        </EmptyState>
      {/if}
    </div>

    <form class="command-bar" onsubmit={send}>
      <div class="bar-top">
        <button
          type="button"
          class="icon-btn"
          aria-pressed={drawerOpen}
          aria-label={t('chat.sessions.toggle')}
          title={t('chat.sessions.toggle')}
          onclick={toggleDrawer}
        >
          <PanelLeft size={16} aria-hidden="true" />
        </button>
        <div class="modes" role="radiogroup" aria-label={t('workshop.modes.label')}>
          {#each visibleModes as m, i (m.id)}
            <button
              type="button"
              role="radio"
              aria-checked={workshop.mode === m.id}
              class="mode"
              class:active={workshop.mode === m.id}
              aria-keyshortcuts={`Alt+${i + 1}`}
              title={`Alt+${i + 1}`}
              onclick={() => (workshop.mode = m.id)}
            >
              {#if m.icon}{@const ModeIcon = m.icon}<ModeIcon size={16} weight={workshop.mode === m.id ? 'fill' : 'regular'} aria-hidden="true" />{/if}
              {t(`workshop.modes.${m.id}`)}
            </button>
          {/each}
        </div>
        {#if view?.top}{@render view.top()}{/if}
      </div>

      {#if view?.controls}{@render view.controls()}{/if}
      {#if workshop.error}<p class="note note-error" role="alert">{workshop.error}</p>{/if}

      {#if view?.input}
        {@render view.input()}
      {:else if view}
        {@const SubmitIcon = view.submitIcon}
        <div class="input-row">
          <label class="sr-only" for="workshop-input">{t('workshop.inputLabel')}</label>
          <div class="prompts">
            <textarea
              id="workshop-input"
              rows={view.promptRows ?? 4}
              bind:value={workshop.prompt}
              onkeydown={onInputKey}
              placeholder={view.placeholder}
              maxlength={view.maxlength}
              aria-keyshortcuts={sendKeyAria(settings.value.send_key)}
            ></textarea>
            {#if view.belowPrompt}{@render view.belowPrompt()}{/if}
          </div>
          {#if view.stop}
            <button type="button" class="btn" onclick={view.stop.run}>
              <Square size={14} aria-hidden="true" />{view.stop.label}
            </button>
          {:else}
            <button type="submit" class="btn btn-primary" disabled={!view.canSend} title={sendKeyHint(settings.value.send_key)}>
              <SubmitIcon size={16} aria-hidden="true" />{view.submitLabel}
            </button>
          {/if}
        </div>
      {/if}
    </form>
  </div>
</div>

<!-- The panel of the open mode puts its pieces into the frame above. -->
<ViewHost load={active?.load} />
