<script lang="ts">
  /** `value` in 0..1; `null` = indeterminate (e.g. verifying a checksum). */
  let { value, label, tone = 'busy' }: { value: number | null; label: string; tone?: 'busy' | 'ok' | 'error' | 'muted' } =
    $props();
  let pct = $derived(value === null ? null : Math.max(0, Math.min(100, Math.round(value * 100))));
</script>

<div
  class="track tone-{tone}"
  role="progressbar"
  aria-label={label}
  aria-valuemin={0}
  aria-valuemax={100}
  aria-valuenow={pct ?? undefined}
>
  <div class="fill" class:indeterminate={pct === null} style:width={pct === null ? '35%' : `${pct}%`}></div>
</div>

<style>
  .track {
    position: relative;
    height: 6px;
    border-radius: 3px;
    background: var(--surface-3);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    border-radius: 3px;
    background: var(--busy);
    transition: width var(--dur-mid) var(--ease);
  }

  .tone-ok .fill {
    background: var(--ok);
  }

  .tone-error .fill {
    background: var(--error);
  }

  .tone-muted .fill {
    background: var(--text-faint);
  }

  .indeterminate {
    position: absolute;
    animation: slide 1.2s var(--ease) infinite;
  }

  @keyframes slide {
    from {
      left: -35%;
    }
    to {
      left: 100%;
    }
  }
</style>
