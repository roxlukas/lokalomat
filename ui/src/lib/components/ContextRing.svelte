<script lang="ts">
  import { getLocale, t } from '../i18n/index.svelte';
  import { formatPercent } from '../format';
  import { arcs, formatTokens, RING_C, type RingState } from '../contextRing';

  /**
   * Small ring under the chat input (ADR 0041): the used part of the context
   * window and, in a second colour, what the typed text will add. The tooltip
   * says it in one sentence; Expert mode adds the numbers and the breakdown.
   */
  let { ring, expert = false }: { ring: RingState; expert?: boolean } = $props();

  let open = $state(false);
  let a = $derived(arcs(ring));
  let loc = $derived(getLocale());
  let total = $derived(ring.usedShare + ring.pendingShare);
  let summary = $derived(t('chat.context.summary', { percent: formatPercent(ring.usedShare, loc) }));
  let parts = $derived.by(() => {
    const u = ring.usage;
    if (!u) return [];
    const free = Math.max(0, ring.size - ring.used);
    return [
      { key: 'messages', tokens: u.messages + u.completion_tokens },
      { key: 'system', tokens: u.system },
      { key: 'tools', tokens: u.tools },
      { key: 'free', tokens: free },
    ].filter((p) => p.tokens > 0 || p.key === 'free');
  });
  let title = $derived(
    [
      summary,
      ring.pending > 0 ? t('chat.context.pending', { tokens: formatTokens(ring.pending, loc) }) : '',
    ]
      .filter(Boolean)
      .join(' '),
  );
</script>

<div
  class="ctx"
  role="group"
  onmouseenter={() => (open = true)}
  onmouseleave={() => (open = false)}
  onfocusin={() => (open = true)}
  onfocusout={() => (open = false)}
>
  <button type="button" class="ring" aria-label={t('chat.context.label')} aria-describedby="ctx-tip" onclick={() => (open = !open)}>
    <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
      <circle class="bg" cx="8" cy="8" r="6.5" />
      <circle
        class="used {ring.level}"
        cx="8"
        cy="8"
        r="6.5"
        stroke-dasharray={a.used.array}
        stroke-dashoffset={a.used.offset}
        transform="rotate(-90 8 8)"
      />
      {#if ring.pendingShare > 0}
        <circle
          class="pending"
          cx="8"
          cy="8"
          r="6.5"
          stroke-dasharray={a.pending.array}
          stroke-dashoffset={a.pending.offset}
          transform="rotate(-90 8 8)"
        />
      {/if}
    </svg>
    {#if expert}<span class="pct {ring.totalLevel}">{formatPercent(total, loc)}</span>{/if}
  </button>
  <div id="ctx-tip" class="tip" role="tooltip" class:open>
    <p>{title}</p>
    {#if ring.level === 'crit'}<p class="full">{t('chat.context.full')}</p>{/if}
    {#if expert}
      <p class="numbers {ring.totalLevel}">
        {t('chat.context.numbers', {
          used: formatTokens(ring.used, loc),
          size: formatTokens(ring.size, loc),
          percent: formatPercent(ring.usedShare, loc),
        })}
      </p>
      {#if parts.length}
        <div class="bar" aria-hidden="true">
          {#each parts as p (p.key)}
            <span class="seg part-{p.key}" style:flex-grow={p.tokens}></span>
          {/each}
        </div>
        <ul>
          {#each parts as p (p.key)}
            <li>
              <span class="dot part-{p.key}"></span>{t(`chat.context.parts.${p.key}`)}
              <span class="n">{formatTokens(p.tokens, loc)} · {formatPercent(p.tokens / ring.size, loc)}</span>
            </li>
          {/each}
        </ul>
        <p class="hint">{t('chat.context.estimate')}</p>
      {/if}
    {/if}
  </div>
</div>

<style>
  .ctx {
    position: relative;
    display: inline-flex;
    align-items: center;
  }
  .ring {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: 2px;
    border: 0;
    border-radius: var(--radius-pill);
    background: none;
    color: var(--text-faint);
    font-size: var(--text-xs);
    cursor: default;
  }
  .ring:focus-visible {
    outline: 2px solid var(--focus);
  }
  circle {
    fill: none;
    stroke-width: 2;
    transition: stroke-dashoffset var(--dur-mid) var(--ease);
  }
  .bg {
    stroke: var(--border-strong);
  }
  .used {
    stroke: var(--accent);
  }
  .used.warn,
  .pct.warn,
  .numbers.warn {
    stroke: var(--warn);
    color: var(--warn);
  }
  .used.crit,
  .pct.crit,
  .numbers.crit {
    stroke: var(--error);
    color: var(--error);
  }
  .pending {
    stroke: var(--info);
  }
  .tip {
    display: none;
    position: absolute;
    bottom: calc(100% + var(--space-2));
    right: 0;
    z-index: 20;
    width: 260px;
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    background: var(--surface);
    box-shadow: var(--shadow);
    color: var(--text);
    font-size: var(--text-s);
  }
  .tip.open {
    display: block;
  }
  .tip p {
    margin: 0 0 var(--space-2);
  }
  .tip p:last-child {
    margin-bottom: 0;
  }
  .full {
    color: var(--error);
  }
  .numbers {
    font-variant-numeric: tabular-nums;
  }
  .bar {
    display: flex;
    height: 6px;
    margin-bottom: var(--space-2);
    overflow: hidden;
    border-radius: var(--radius-pill);
  }
  .seg {
    flex-basis: 0;
  }
  ul {
    margin: 0 0 var(--space-2);
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .n {
    margin-left: auto;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: var(--radius-pill);
  }
  .part-messages {
    background: var(--accent);
  }
  .part-system {
    background: var(--ok);
  }
  .part-tools {
    background: var(--busy);
  }
  .part-free {
    background: var(--border-strong);
  }
  .hint {
    color: var(--text-faint);
    font-size: var(--text-xs);
  }
</style>
