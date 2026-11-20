<script lang="ts">
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import Gauge from '@lucide/svelte/icons/gauge';
  import CircleX from '@lucide/svelte/icons/circle-x';
  import CircleHelp from '@lucide/svelte/icons/circle-help';
  import { getLocale, t } from '../i18n/index.svelte';
  import type { BudgetKind, FitReport } from '../ipc';
  import { formatMb, formatPercent } from '../format';

  let { report, kind = 'gpu', compact = false }: { report: FitReport | null; kind?: BudgetKind; compact?: boolean } =
    $props();

  const icons = { fits: CircleCheck, tight: CircleAlert, offload: Gauge, wont_run: CircleX };

  let fit = $derived(report?.fit ?? 'unknown');
  let Icon = $derived(report ? icons[report.fit] : CircleHelp);
  let details = $derived.by(() => {
    if (!report) return t('fit.unknownHint');
    const loc = getLocale();
    let text = t('fit.numbers', {
      estimate: formatMb(report.estimate_mb, loc),
      budget: formatMb(report.budget_mb, loc),
      kind: t(`fit.budgetKind.${kind}`),
    });
    if (report.headroom >= 0) text += ` · ${t('fit.headroom', { headroom: formatPercent(report.headroom, loc) })}`;
    if (report.fit === 'offload') text += ` · ${t('fit.offloadNumbers', { offload: formatMb(report.offload_mb, loc) })}`;
    return `${t(`fit.hint.${report.fit}`)} ${text}`;
  });
</script>

<span class="fit fit-{fit}" class:compact title={details}>
  <Icon size={14} aria-hidden="true" />
  <span class="label">{t(`fit.${fit}`)}</span>
  <span class="sr-only">. {details}</span>
</span>

<style>
  .fit {
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

  .fit-fits {
    background: var(--ok-bg);
    color: var(--ok);
  }

  .fit-tight {
    background: var(--warn-bg);
    color: var(--warn);
  }

  .fit-offload {
    background: var(--info-bg);
    color: var(--info);
  }

  .fit-wont_run {
    background: var(--error-bg);
    color: var(--error);
  }

  .fit-unknown {
    background: var(--surface-2);
    color: var(--text-muted);
  }

  .compact .label {
    display: none;
  }
</style>
