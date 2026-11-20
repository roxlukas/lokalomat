<script lang="ts">
  import Zap from '@lucide/svelte/icons/zap';
  import Gauge from '@lucide/svelte/icons/gauge';
  import Turtle from '@lucide/svelte/icons/turtle';
  import Snail from '@lucide/svelte/icons/snail';
  import CircleX from '@lucide/svelte/icons/circle-x';
  import { getLocale, t } from '../i18n/index.svelte';
  import type { BudgetKind, FitReport, SpeedReport } from '../ipc';
  import { formatMb } from '../format';
  import FitBadge from './FitBadge.svelte';

  /**
   * One badge for "how well will it run here" (ADR 0016): the speed level when
   * it runs, red when it needs a graphics card or does not fit in memory.
   * Numbers stay in the tooltip; `showValue` (Expert mode) adds them inline.
   */
  let {
    speed,
    fit = null,
    kind = 'gpu',
    showValue = false,
  }: { speed: SpeedReport | null | undefined; fit?: FitReport | null; kind?: BudgetKind; showValue?: boolean } =
    $props();

  const icons = { fast: Zap, usable: Gauge, slow: Turtle, too_slow: Snail, needs_gpu: CircleX, wont_run: CircleX };

  let level = $derived.by(() => {
    if (!speed) return null;
    if (speed.reason === 'needs_gpu') return 'needs_gpu' as const;
    if (speed.reason === 'not_enough_memory') return 'wont_run' as const;
    return speed.speed;
  });
  let Icon = $derived(level ? icons[level] : CircleX);

  function num(n: number): string {
    const digits = n >= 10 ? 0 : 1;
    return n.toLocaleString(getLocale(), { maximumFractionDigits: digits, minimumFractionDigits: 0 });
  }

  let short = $derived(speed && speed.value > 0 ? t(`speed.short.${speed.unit}`, { value: num(speed.value) }) : '');

  let details = $derived.by(() => {
    if (!speed || !level) return '';
    let text = t(`speed.hint.${level}`);
    if (speed.value > 0) {
      const numbers = t(`speed.numbers.${speed.unit}`, {
        value: num(speed.value),
        prompt: num(speed.prompt_tokens_per_s ?? 0),
      });
      text += ` ${t('speed.estimate', { numbers, device: t(`speed.device.${speed.device}`) })}`;
    }
    if (fit) {
      text += ` ${t('fit.numbers', {
        estimate: formatMb(fit.estimate_mb, getLocale()),
        budget: formatMb(fit.budget_mb, getLocale()),
        kind: t(`fit.budgetKind.${kind}`),
      })}`;
    }
    return text;
  });
</script>

{#if level}
  <span class="speed speed-{level}" title={details}>
    <Icon size={14} aria-hidden="true" />
    <span class="label">{t(`speed.${level}`)}{#if showValue && short}<span class="value">{` · ${short}`}</span>{/if}</span>
    <span class="sr-only">. {details}</span>
  </span>
{:else}
  <FitBadge report={fit} {kind} />
{/if}

<style>
  .speed {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 1px 8px 1px 6px;
    border-radius: 999px;
    font-size: var(--text-xs);
    font-weight: 600;
    white-space: nowrap;
    cursor: help;
  }

  .value {
    font-weight: 400;
  }

  .speed-fast {
    background: var(--ok-bg);
    color: var(--ok);
  }

  .speed-usable {
    background: var(--speed-usable-bg);
    color: var(--speed-usable);
  }

  .speed-slow {
    background: var(--speed-slow-bg);
    color: var(--speed-slow);
  }

  .speed-too_slow,
  .speed-needs_gpu,
  .speed-wont_run {
    background: var(--error-bg);
    color: var(--error);
  }
</style>
