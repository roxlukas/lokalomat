<script lang="ts">
  import { onDestroy, onMount } from 'svelte';

  /** A small character animation ("the model is working"), like the one in Claude. */
  const FRAMES = ['·', '✢', '✳', '✶', '✻', '✽', '✻', '✶', '✳', '✢'];
  const STEP_MS = 140;

  let index = $state(0);
  let timer: ReturnType<typeof setInterval> | undefined;

  onMount(() => {
    // Respect "reduce motion": a still glyph instead of the animation.
    if (globalThis.matchMedia?.('(prefers-reduced-motion: reduce)').matches) {
      index = 4;
      return;
    }
    timer = setInterval(() => (index = (index + 1) % FRAMES.length), STEP_MS);
  });

  onDestroy(() => {
    if (timer) clearInterval(timer);
  });
</script>

<span class="spinner" aria-hidden="true">{FRAMES[index]}</span>

<style>
  .spinner {
    display: inline-block;
    width: 1.2em;
    color: var(--accent);
    font-family: var(--font-mono);
    text-align: center;
  }
</style>
