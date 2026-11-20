<script lang="ts">
  import Copy from '@lucide/svelte/icons/copy';
  import Check from '@lucide/svelte/icons/check';
  import Square from '@lucide/svelte/icons/square';
  import Brain from '@lucide/svelte/icons/brain';
  import { openUrl } from '@tauri-apps/plugin-opener';
  import { getLocale, t } from '../i18n/index.svelte';
  import { inTauri, type Message } from '../ipc';
  import { renderMarkdown } from '../markdown';
  import { isFinished } from '../chat.svelte';
  import { models } from '../stores.svelte';
  import { logToApp } from '../errors';
  import { storedErrorParams } from '../feedback';
  import SlotHost from './SlotHost.svelte';
  import { settings } from '../settings.svelte';

  let { message, onCancel }: { message: Message; onCancel: (id: number) => void } = $props();

  let copied = $state(false);
  let html = $derived(message.role === 'assistant' ? renderMarkdown(message.content) : '');
  let finished = $derived(isFinished(message));
  let modelName = $derived.by(() => {
    const m = message.model_id ? models.byId(message.model_id) : undefined;
    const v = m?.variants.find((x) => x.id === message.variant_id);
    return m ? `${m.name}${v ? ` · ${v.label}` : ''}` : (message.model_id ?? '');
  });
  let errorText = $derived.by(() => {
    if (!message.error) return '';
    const code = message.error.split(':')[0]!.trim();
    const key = `chat.errors.${code}`;
    const text = t(key, storedErrorParams(message.error));
    return text === key ? t('chat.errors.engine_error') : text;
  });

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch (e) {
      logToApp('warn', `clipboard write failed: ${String(e)}`);
    }
  }

  /** Links in model output open in the system browser, never inside the app. */
  function onBodyClick(e: MouseEvent) {
    const a = (e.target as HTMLElement).closest('a');
    if (!a) return;
    e.preventDefault();
    const href = a.getAttribute('href') ?? '';
    if (!/^https?:\/\//i.test(href)) return;
    if (inTauri) void openUrl(href).catch((err) => logToApp('warn', `open link failed: ${String(err)}`));
    else window.open(href, '_blank', 'noopener');
  }

  /** Adds a copy button to every code block (buttons are ours, not model HTML). */
  function codeCopy(node: HTMLElement, _content: string) {
    const decorate = () => {
      for (const pre of node.querySelectorAll('pre')) {
        if (pre.querySelector('.code-copy')) continue;
        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'code-copy';
        btn.textContent = t('chat.copyCode');
        btn.addEventListener('click', () => void copy(pre.querySelector('code')?.textContent ?? pre.textContent ?? ''));
        pre.appendChild(btn);
      }
    };
    decorate();
    return { update: decorate };
  }

  function time(ts: number): string {
    return new Date(ts * 1000).toLocaleTimeString(getLocale(), { hour: '2-digit', minute: '2-digit' });
  }
</script>

{#if message.role === 'user'}
  <article class="user" aria-label={t('chat.you')}>
    <p class="text">{message.content}</p>
    <span class="time">{time(message.created_at)}</span>
  </article>
{:else}
  <article class="assistant" aria-label={t('chat.assistant')} aria-busy={!finished}>
    <header>
      <span class="model">{modelName}</span>
      <span class="status status-{message.status}">
        {#if !finished}<span class="dot" aria-hidden="true"></span>{/if}
        {t(`chat.status.${message.status}`)}
      </span>
      {#if message.tokens_per_second && finished && settings.value.expert}
        <span class="meta">{t('chat.speed', { tps: message.tokens_per_second.toLocaleString(getLocale(), { maximumFractionDigits: 1 }), tokens: message.tokens ?? '?' })}</span>
      {/if}
    </header>

    {#if message.reasoning}
      <details class="reasoning" open={!finished && !message.content}>
        <summary><Brain size={14} aria-hidden="true" />{t('chat.reasoning')}</summary>
        <p>{message.reasoning}</p>
      </details>
    {/if}

    {#if message.tool_calls?.length}
      <div class="tools">
        {#each message.tool_calls as call (call.id)}
          <SlotHost name="tool-call" props={{ call }} />
        {/each}
      </div>
    {/if}

    {#if message.content}
      <!-- Sanitised by DOMPurify in renderMarkdown(); clicks on links are intercepted. -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="body" onclick={onBodyClick} use:codeCopy={html}>{@html html}</div>
    {:else if !finished}
      <p class="waiting">{t(`chat.waiting.${message.status}`)}</p>
    {/if}

    {#if message.status === 'failed'}
      <p class="error" role="alert">{errorText}</p>
    {/if}

    <footer>
      {#if !finished}
        <button type="button" class="btn small" onclick={() => onCancel(message.id)}>
          <Square size={12} aria-hidden="true" />{t('chat.stop')}
        </button>
      {:else if message.content}
        <button type="button" class="btn small ghost" onclick={() => copy(message.content)}>
          {#if copied}<Check size={14} aria-hidden="true" />{t('chat.copied')}{:else}<Copy size={14} aria-hidden="true" />{t('chat.copy')}{/if}
        </button>
      {/if}
    </footer>
  </article>
{/if}

<style>
  .user {
    align-self: flex-end;
    max-width: min(680px, 85%);
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-l) var(--radius-l) var(--radius-s) var(--radius-l);
    background: var(--surface-3);
  }

  .user .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .time {
    display: block;
    margin-top: 2px;
    color: var(--text-faint);
    font-size: var(--text-xs);
    text-align: right;
  }

  .assistant {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    max-width: 860px;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-l);
    background: var(--surface);
  }

  header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
  }

  .model {
    font-weight: 600;
    color: var(--text-muted);
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--text-faint);
  }

  .status-running,
  .status-loading_model,
  .status-queued {
    color: var(--busy);
  }

  .status-failed {
    color: var(--error);
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
    animation: blink 1s ease-in-out infinite;
  }

  @keyframes blink {
    50% {
      opacity: 0.25;
    }
  }

  .meta {
    margin-left: auto;
    color: var(--text-faint);
    font-variant-numeric: tabular-nums;
  }

  .reasoning {
    padding: var(--space-2) var(--space-3);
    border-left: 3px solid var(--border-strong);
    background: var(--bg);
    border-radius: var(--radius-s);
    font-size: var(--text-s);
    color: var(--text-muted);
  }

  .reasoning summary {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    cursor: pointer;
  }

  .reasoning p {
    margin-top: var(--space-2);
    white-space: pre-wrap;
  }

  .tools {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .waiting {
    color: var(--text-muted);
    font-size: var(--text-s);
  }

  .error {
    color: var(--error);
    font-size: var(--text-s);
  }

  footer {
    display: flex;
    gap: var(--space-2);
  }

  footer:empty {
    display: none;
  }

  .btn.small {
    min-height: 28px;
    padding: 0 var(--space-3);
    font-size: var(--text-s);
  }

  .btn.ghost {
    border-color: transparent;
    color: var(--text-muted);
  }

  .body {
    overflow-wrap: anywhere;
    line-height: 1.6;
  }

  .body :global(p) {
    margin: 0 0 var(--space-2);
  }

  .body :global(:is(ul, ol)) {
    margin: 0 0 var(--space-2);
    padding-left: var(--space-6);
  }

  .body :global(:is(h1, h2, h3, h4)) {
    margin: var(--space-3) 0 var(--space-2);
    font-size: var(--text-l);
  }

  .body :global(code) {
    padding: 1px 4px;
    border-radius: 4px;
    background: var(--surface-2);
    font-family: var(--font-mono);
    font-size: 0.92em;
  }

  .body :global(pre) {
    position: relative;
    margin: 0 0 var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    background: var(--bg);
    overflow-x: auto;
  }

  .body :global(pre code) {
    padding: 0;
    background: none;
  }

  .body :global(.code-copy) {
    position: absolute;
    top: var(--space-2);
    right: var(--space-2);
    padding: 2px 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-s);
    background: var(--surface);
    color: var(--text-muted);
    font-size: var(--text-xs);
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }

  .body :global(pre:hover .code-copy),
  .body :global(.code-copy:focus-visible) {
    opacity: 1;
  }

  .body :global(table) {
    border-collapse: collapse;
    margin-bottom: var(--space-2);
  }

  .body :global(:is(th, td)) {
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--border);
  }

  .body :global(a) {
    color: var(--focus);
  }
</style>
