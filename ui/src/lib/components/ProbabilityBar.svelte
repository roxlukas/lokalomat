<script lang="ts">
  /** One answer of a decision with its probability (0..1); `top` marks the chosen answer. */
  let { label, p, top = false }: { label: string; p: number; top?: boolean } = $props();
  let pct = $derived(Math.max(0, Math.min(100, Math.round(p * 100))));
</script>

<div class="row" class:top>
  <span class="label">{label}</span>
  <div class="track" role="meter" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct} aria-valuetext={`${pct}%`}>
    <div class="fill" style:width={`${pct}%`}></div>
  </div>
  <span class="pct">{pct}%</span>
</div>

<style>
  .row {
    display: grid;
    grid-template-columns: minmax(80px, 35%) 1fr 3.5em;
    gap: var(--space-2);
    align-items: center;
    font-size: var(--text-s);
    color: var(--text-muted);
  }

  .row.top {
    color: var(--text);
    font-weight: 600;
  }

  .label {
    overflow-wrap: anywhere;
  }

  .track {
    height: 8px;
    border-radius: var(--radius-pill);
    background: var(--surface-3);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    border-radius: var(--radius-pill);
    background: var(--text-faint);
    transition: width var(--dur-mid) var(--ease);
  }

  .top .fill {
    background: var(--accent);
  }

  .pct {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
</style>
