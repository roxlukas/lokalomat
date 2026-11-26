<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import ChatCircle from 'phosphor-svelte/lib/ChatCircleIcon';
  import Microphone from 'phosphor-svelte/lib/MicrophoneIcon';
  import SpeakerHigh from 'phosphor-svelte/lib/SpeakerHighIcon';
  import PaintBrush from 'phosphor-svelte/lib/PaintBrushIcon';
  import SquaresFour from 'phosphor-svelte/lib/SquaresFourIcon';
  import MusicNotes from 'phosphor-svelte/lib/MusicNotesIcon';
  import Scales from 'phosphor-svelte/lib/ScalesIcon';
  import PaperPlaneRight from 'phosphor-svelte/lib/PaperPlaneRightIcon';
  import Stop from 'phosphor-svelte/lib/StopIcon';
  import Gear from 'phosphor-svelte/lib/GearSixIcon';
  import WifiHigh from 'phosphor-svelte/lib/WifiHighIcon';
  import WifiSlash from 'phosphor-svelte/lib/WifiSlashIcon';
  import Copy from 'phosphor-svelte/lib/CopyIcon';
  import BrandMark from '../lib/components/BrandMark.svelte';
  import { t } from '../lib/i18n/index.svelte';
  import { renderMarkdown } from '../lib/markdown';
  import { clock, encodeWav, toMono } from '../lib/media';
  import { readLocal, writeLocal } from '../lib/storage';
  import { api, describe, isOffline, toBase64, type Computers, type MobileStatus, type Transcription } from './api';
  import { chats } from './chats.svelte';
  import MobileChatBar from './MobileChatBar.svelte';
  import MobileDecision from './MobileDecision.svelte';
  import MobileGallery from './MobileGallery.svelte';
  import MobileLocalChat from './MobileLocalChat.svelte';
  import MobileSettings from './MobileSettings.svelte';
  import MobileWelcome from './MobileWelcome.svelte';
  import MobileImage from './MobileImage.svelte';
  import MobileMusic from './MobileMusic.svelte';
  import MobileVoice from './MobileVoice.svelte';
  import { local } from './local';
  import { phone, type Reuse } from './phone.svelte';
  import ModelPicker from './ModelPicker.svelte';
  import PairScreen from './PairScreen.svelte';
  import ReportAnswer from './ReportAnswer.svelte';
  import { source } from './source.svelte';

  const TABS = ['chat', 'record', 'voice', 'image', 'music', 'decision', 'gallery'] as const;
  type Tab = (typeof TABS)[number];
  const ICONS = { chat: ChatCircle, record: Microphone, voice: SpeakerHigh, image: PaintBrush, music: MusicNotes, decision: Scales, gallery: SquaresFour };
  /** The computer offers these only in Expert mode (ADR 0027): the tab shows when it does. */
  const EXPERT_TABS: Tab[] = ['decision'];
  /** The same lists as on the computer (transcription and the voice Inspector). */
  const STT_LANGUAGES = ['auto', 'pl', 'en', 'de', 'uk', 'fr', 'es'] as const;
  /** The tab of each kind the Gallery can "use again". */
  const REUSE_TAB: Record<Reuse['kind'], Tab> = { image: 'image', tts: 'voice', music: 'music' };

  let status = $state<MobileStatus | null>(null);
  let tab = $state<Tab>('chat');
  let modes = $state<string[]>([]);
  let error = $state('');
  let busy = $state(false);

  // chat (the list of chats is kept on the phone: chats.svelte.ts)
  let input = $state('');
  // record
  let recorder: MediaRecorder | null = null;
  let recording = $state(false);
  let transcript = $state<Transcription | null>(null);
  let copied = $state(false);
  let language = $state('auto');
  // model chosen per mode ("" = the computer's default)
  let chatModel = $state('');
  let sttModel = $state('');

  // connection to the computer: null = not checked yet
  const CHECK_EVERY_MS = 15_000;
  let online = $state<boolean | null>(null);
  let timer: ReturnType<typeof setInterval> | undefined;
  /** The pairing screen (settings, "Dodaj komputer", or the welcome screen). */
  let pairing = $state(false);
  let settings = $state(false);
  /** Every paired computer; the dropdown and the settings list show them. */
  let computers = $state<Computers>({ active: null, list: [] });
  /** The first-start choice was made (or the phone was already paired). */
  const ONBOARDED_KEY = 'lokalsi.mobile.onboarded';
  let onboarded = $state(readLocal<boolean>(ONBOARDED_KEY, false));
  let welcome = $derived(status !== null && !status.paired && source.localAvailable && !onboarded);

  /** Where the chat runs now: this phone or the paired computer (ADR 0038). */
  let where = $derived(source.current(status?.paired === true));
  let onPhone = $derived(where === 'local');

  onMount(async () => {
    source.localAvailable = await local.available();
    status = await api.status().catch(() => ({ paired: false, id: null, computer_name: null, address: null }));
    await refreshComputers();
    if (status.paired) finishOnboarding();
    if (status.paired && where === 'computer') void connect();
    document.addEventListener('visibilitychange', onVisibility);
  });

  onDestroy(() => {
    stopChecks();
    document.removeEventListener('visibilitychange', onVisibility);
  });

  async function connect() {
    await check();
    startChecks();
  }

  /** Asks the computer for its info; updates the connection icon and modes. */
  async function check() {
    try {
      const info = await api.info();
      modes = info.modes;
      phone.expert = info.expert === true;
      phone.pro = info.pro === true;
      if (!visible(tab)) tab = 'chat';
      if (online === false) error = '';
      online = true;
    } catch (e) {
      if (isOffline(e)) online = false;
      else error = describe(e);
    }
  }

  // Checks only while the app is on screen (no battery use in the background).
  function startChecks() {
    stopChecks();
    if (document.visibilityState === 'visible') timer = setInterval(() => void check(), CHECK_EVERY_MS);
  }

  function stopChecks() {
    if (timer) clearInterval(timer);
    timer = undefined;
  }

  function onVisibility() {
    // The launcher icon follows the edition; switched in the background so it never interrupts work.
    if (document.visibilityState === 'hidden') void api.launcherIcon(status?.paired === true && phone.pro).catch(() => {});
    if (!status?.paired || onPhone) return;
    if (document.visibilityState === 'visible') {
      void check();
      startChecks();
    } else {
      stopChecks();
    }
  }

  async function run(fn: () => Promise<void>) {
    error = '';
    busy = true;
    try {
      await fn();
      online = true;
    } catch (e) {
      if (isOffline(e)) online = false;
      error = describe(e);
    } finally {
      busy = false;
    }
  }

  async function refreshComputers() {
    computers = await api.computers().catch(() => ({ active: null, list: [] }));
    // No computer left: back to the free (purple) mark.
    if (computers.list.length === 0) phone.pro = false;
  }

  function finishOnboarding() {
    onboarded = true;
    writeLocal(ONBOARDED_KEY, true);
  }

  async function paired(s: MobileStatus) {
    status = s;
    pairing = false;
    finishOnboarding();
    source.set('computer');
    chats.startNew();
    online = null;
    await refreshComputers();
    void connect();
  }

  /** The dropdown: "local" or the id of a paired computer. */
  async function pickSource(e: Event) {
    const value = (e.currentTarget as HTMLSelectElement).value;
    error = '';
    if (value === 'local') {
      source.set('local');
      // Nothing to ask the computer while the chat runs on the phone.
      stopChecks();
      return;
    }
    source.set('computer');
    if (value !== status?.id) {
      stopChecks();
      chats.startNew();
      online = null;
      modes = [];
      status = await api.computerSelect(value).catch(() => status);
      await refreshComputers();
    }
    void connect();
  }

  /** A computer was forgotten in the settings: reload what depends on it. */
  async function computersChanged() {
    status = await api.status();
    await refreshComputers();
    chats.startNew();
    online = null;
    stopChecks();
    if (!status.paired) {
      if (source.localAvailable) source.set('local');
    } else if (where === 'computer') {
      void connect();
    }
  }

  function openModels() {
    settings = false;
    source.set('local');
    source.openModels = true;
    stopChecks();
  }

  async function send() {
    const text = input.trim();
    if (!text) return;
    input = '';
    chats.messages = [...chats.messages, { role: 'user', content: text }];
    chats.save();
    const id = chats.currentId;
    await run(async () => {
      const answer = await api.chat(chats.messages, chatModel || null);
      // The user may have opened another chat meanwhile: the reply belongs to this one.
      if (chats.currentId !== id) return;
      chats.messages = [...chats.messages, { role: 'assistant', content: answer }];
      chats.save();
    });
  }

  async function toggleRecording() {
    if (recording && recorder) {
      recorder.stop();
      return;
    }
    error = '';
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const chunks: Blob[] = [];
      recorder = new MediaRecorder(stream);
      recorder.ondataavailable = (e) => chunks.push(e.data);
      recorder.onstop = () => {
        recording = false;
        stream.getTracks().forEach((tr) => tr.stop());
        const blob = new Blob(chunks, { type: recorder?.mimeType || 'audio/webm' });
        // WebView records WebM/Opus, which the computer cannot decode: send WAV.
        void run(async () => {
          const ctx = new AudioContext();
          try {
            const audio = await ctx.decodeAudioData(await blob.arrayBuffer());
            const channels = Array.from({ length: audio.numberOfChannels }, (_, i) => audio.getChannelData(i));
            const wav = encodeWav(toMono(channels), audio.sampleRate);
            await sendAudio(new Blob([wav.slice().buffer], { type: 'audio/wav' }), 'wav');
          } finally {
            await ctx.close().catch(() => undefined);
          }
        });
      };
      recorder.start();
      recording = true;
    } catch {
      error = t('mobile.record.noMic');
    }
  }

  async function pickFile(e: Event) {
    const file = (e.currentTarget as HTMLInputElement).files?.[0];
    if (!file) return;
    const ext = file.name.split('.').pop()?.toLowerCase() ?? 'audio';
    await sendAudio(file, ext);
  }

  async function sendAudio(blob: Blob, ext: string) {
    transcript = null;
    copied = false;
    await run(async () => {
      const bytes = new Uint8Array(await blob.arrayBuffer());
      const result = await api.transcribe(toBase64(bytes), ext, language === 'auto' ? null : language, sttModel || null);
      transcript = result;
    });
  }

  async function copyTranscript() {
    if (!transcript) return;
    await navigator.clipboard
      .writeText(transcript.text)
      .then(() => (copied = true))
      .catch(() => undefined);
  }

  function choose(next: Tab) {
    tab = next;
    error = '';
  }

  /** "Użyj ponownie" in the Gallery: the screen of that mode takes the settings. */
  function reuse(r: Reuse) {
    phone.reuse = r;
    choose(REUSE_TAB[r.kind]);
  }

  function available(tab: Tab): boolean {
    const need = { chat: 'chat', record: 'stt', voice: 'tts', image: 'image', music: 'music', decision: 'decision', gallery: '' }[tab];
    return !need || modes.includes(need);
  }

  function visible(tab: Tab): boolean {
    return !EXPERT_TABS.includes(tab) || modes.includes(tab);
  }

  let tabs = $derived(TABS.filter(visible));
</script>

{#if status === null}
  <p class="center muted">{t('mobile.loading')}</p>
{:else if !status.paired && !source.localAvailable}
  <PairScreen onPaired={paired} />
{:else if welcome}
  <MobileWelcome
    onPhone={() => (finishOnboarding(), source.set('local'))}
    onComputer={() => (finishOnboarding(), (pairing = true))}
  />
  {#if pairing}
    <PairScreen overlay onPaired={paired} onClose={() => (pairing = false)} />
  {/if}
{:else}
  <div class="app">
    <header>
      <div class="who">
        <label class="source">
          <span class="sr-only">{t('mobile.source.label')}</span>
          <select value={onPhone ? 'local' : (status.id ?? '')} onchange={pickSource}>
            {#if source.localAvailable}<option value="local">{t('mobile.source.local')}</option>{/if}
            {#each computers.list as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
          </select>
        </label>
        {#if !onPhone && status.paired}
          {#if online === false}
            <span class="link offline small" role="status" title={t('mobile.link.offlineHint')}>
              <WifiSlash size={16} aria-hidden="true" />{t('mobile.link.offline')}
            </span>
          {:else}
            <span class="muted small address">{status.address}</span>
          {/if}
        {/if}
      </div>
      <div class="tools">
        {#if !onPhone && status.paired && online}
          <span class="link online" role="status" aria-label={t('mobile.link.online')} title={t('mobile.link.online')}>
            <WifiHigh size={18} aria-hidden="true" />
          </span>
        {/if}
        <button type="button" class="icon-btn" aria-label={t('mobile.settings.title')} title={t('mobile.settings.title')} onclick={() => (settings = true)}>
          <Gear size={22} aria-hidden="true" />
        </button>
      </div>
      <div class="brand">
        <BrandMark size={20} pro={phone.pro} />
        <span>{t('app.shortName')}</span>
      </div>
    </header>

    {#if pairing}
      <PairScreen overlay onPaired={paired} onClose={() => (pairing = false)} />
    {/if}

    {#if settings}
      <MobileSettings
        {computers}
        canUsePhone={source.localAvailable}
        onClose={() => (settings = false)}
        onAdd={() => ((settings = false), (pairing = true))}
        onModels={openModels}
        onChanged={computersChanged}
      />
    {/if}

    <main class:solo={onPhone}>
      {#if onPhone}
        <MobileLocalChat />
      {:else}
      {#if error}<p class="note note-error" role="alert">{error}</p>{/if}
      {#if online === false}
        <!-- the modes are unknown while the computer cannot be reached: say that, not "no model" -->
        <p class="note note-warn" role="status">{t('mobile.link.offlineHint')}</p>
      {:else if online && !available(tab)}
        <p class="note note-info">{t('mobile.modeMissing')}</p>
      {/if}

      {#if tab === 'chat'}
        <MobileChatBar {busy}><ModelPicker mode="chat" bind:value={chatModel} /></MobileChatBar>
        <div class="stream" aria-live="polite">
          {#each chats.messages as m, i (i)}
            <div class="msg {m.role}">
              <!-- eslint-disable-next-line svelte/no-at-html-tags -- sanitised by renderMarkdown (DOMPurify) -->
              {#if m.role === 'assistant'}{@html renderMarkdown(m.content)}{:else}{m.content}{/if}
            </div>
            {#if m.role === 'assistant'}<ReportAnswer answer={m.content} where={status.computer_name ?? 'computer'} />{/if}
          {/each}
          {#if busy}<p class="muted small">{t('mobile.working')}</p>{/if}
        </div>
        <form class="bar" onsubmit={(e) => (e.preventDefault(), void send())}>
          <textarea rows="4" bind:value={input} placeholder={t('mobile.chat.placeholder')} aria-label={t('mobile.chat.placeholder')}></textarea>
          <button type="submit" class="btn btn-primary" disabled={busy || !input.trim()} aria-label={t('mobile.send')}>
            <PaperPlaneRight size={18} aria-hidden="true" />
          </button>
        </form>
      {:else if tab === 'record'}
        <div class="panel">
          <ModelPicker mode="stt" bind:value={sttModel} />
          <label class="field">
            <span class="small muted">{t('workshop.stt.language')}</span>
            <select bind:value={language}>
              {#each STT_LANGUAGES as l (l)}<option value={l}>{t(`workshop.stt.languages.${l}`)}</option>{/each}
            </select>
          </label>
          <button type="button" class="btn btn-primary big" class:rec={recording} onclick={toggleRecording} disabled={busy}>
            {#if recording}<Stop size={20} aria-hidden="true" />{t('mobile.record.stop')}{:else}<Microphone size={20} aria-hidden="true" />{t('mobile.record.start')}{/if}
          </button>
          <label class="file">
            <span class="small">{t('mobile.record.file')}</span>
            <input type="file" accept="audio/*,video/mp4" onchange={pickFile} disabled={busy} />
          </label>
          {#if busy}<p class="muted small">{t('mobile.working')}</p>{/if}
          {#if transcript}
            {#if transcript.segments.length > 1}
              <ol class="segments">
                {#each transcript.segments as s, i (i)}
                  <li><span class="time">{clock(s.start_ms)}</span>{s.text}</li>
                {/each}
              </ol>
            {:else}
              <p class="result">{transcript.text}</p>
            {/if}
            <button type="button" class="btn" onclick={copyTranscript} disabled={!transcript.text}>
              <Copy size={18} aria-hidden="true" />{copied ? t('mobile.record.copied') : t('mobile.record.copy')}
            </button>
          {/if}
        </div>
      {:else if tab === 'voice'}
        <MobileVoice />
      {:else if tab === 'image'}
        <MobileImage />
      {:else if tab === 'music'}
        <MobileMusic />
      {:else if tab === 'decision'}
        <MobileDecision />
      {:else}
        <MobileGallery onReuse={reuse} canReuse={(kind) => available(REUSE_TAB[kind])} />
      {/if}
      {/if}
    </main>

    {#if !onPhone && status.paired}
      <nav aria-label={t('mobile.modes')}>
        {#each tabs as id (id)}
          {@const Icon = ICONS[id]}
          <button type="button" class:active={tab === id} aria-current={tab === id ? 'page' : undefined} onclick={() => choose(id)}>
            <Icon size={20} weight={tab === id ? 'fill' : 'regular'} aria-hidden="true" />
            <span>{t(`mobile.tabs.${id}`)}</span>
          </button>
        {/each}
      </nav>
    {/if}
  </div>
{/if}

<style>
  .center {
    padding: var(--space-10);
    text-align: center;
  }

  .muted {
    color: var(--text-muted);
  }

  .small {
    font-size: var(--text-s);
  }

  .app {
    display: grid;
    grid-template-rows: auto 1fr auto;
    height: 100%;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .who {
    display: grid;
    flex: 1;
    min-width: 0;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-left: var(--space-3);
    color: var(--accent);
  }

  .brand span {
    color: var(--text);
    font-family: var(--font-display);
    font-size: var(--text-m, 1rem);
    font-weight: 700;
    letter-spacing: -0.01em;
  }

  .address {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .link.offline {
    justify-self: start;
    margin-top: 2px;
  }

  .source select {
    max-width: 100%;
    font-weight: 600;
  }

  .tools {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .link {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
  }

  .link.online {
    color: var(--ok);
  }

  .link.offline {
    padding: 2px var(--space-2);
    border-radius: var(--radius-pill);
    background: var(--error-bg);
    color: var(--error);
    font-weight: 600;
  }

  main {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-height: 0;
    padding: var(--space-3) var(--space-4);
    overflow-y: auto;
  }

  /* No tab bar below on the phone chat: keep the input clear of the system bar. */
  main.solo {
    padding-bottom: max(var(--space-3), env(safe-area-inset-bottom, 0px));
  }

  .stream {
    display: grid;
    flex: 1;
    align-content: start;
    gap: var(--space-2);
  }

  .msg {
    max-width: 90%;
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-l);
    overflow-wrap: anywhere;
  }

  .msg.user {
    justify-self: end;
    background: var(--accent-soft);
    white-space: pre-wrap;
  }

  .msg.assistant {
    justify-self: start;
    background: var(--surface);
    border: 1px solid var(--border);
  }

  .bar {
    display: flex;
    gap: var(--space-2);
    align-items: flex-end;
  }

  .bar textarea {
    flex: 1;
    width: 100%;
  }

  .panel {
    display: grid;
    gap: var(--space-3);
  }

  .field {
    display: grid;
    gap: var(--space-1);
  }

  .big {
    min-height: 56px;
    font-size: var(--text-l);
  }

  .rec {
    background: var(--error);
    border-color: var(--error);
  }

  .file {
    display: grid;
    gap: var(--space-1);
  }

  .result {
    padding: var(--space-3);
    border-radius: var(--radius-m);
    background: var(--surface);
    white-space: pre-wrap;
  }

  .segments {
    display: grid;
    gap: var(--space-2);
    max-height: 50vh;
    overflow-y: auto;
    margin: 0;
    padding: var(--space-3);
    border-radius: var(--radius-m);
    background: var(--surface);
    list-style: none;
  }

  .segments .time {
    margin-right: var(--space-2);
    color: var(--text-muted);
    font-family: var(--font-mono);
    font-size: var(--text-s);
  }

  nav {
    display: grid;
    grid-auto-columns: minmax(0, 1fr);
    grid-auto-flow: column;
    border-top: 1px solid var(--border);
    background: var(--surface);
    padding-bottom: env(safe-area-inset-bottom);
  }

  nav button {
    display: grid;
    justify-items: center;
    gap: 2px;
    min-height: 56px;
    padding: var(--space-1) 2px;
    border: 0;
    background: none;
    color: var(--text-muted);
    font-size: var(--text-xs);
  }

  nav button span {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  nav button.active {
    color: var(--accent);
  }
</style>
