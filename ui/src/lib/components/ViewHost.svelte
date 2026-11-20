<script lang="ts">
  import type { Component } from 'svelte';
  import type { Loader } from '../edition/registry';

  /** Loads a component (a registered view or tab) and shows it. */
  let { load }: { load: Loader | undefined } = $props();

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let View = $state<Component<any> | null>(null);

  $effect(() => {
    const loader = load;
    let live = true;
    View = null;
    void loader?.().then((m) => {
      if (live) View = m.default;
    });
    return () => {
      live = false;
    };
  });
</script>

{#if View}
  <View />
{/if}
