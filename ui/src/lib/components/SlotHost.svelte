<script lang="ts">
  import type { Component } from 'svelte';
  import { slotItems, type SlotName } from '../edition/registry';

  /** Renders what the edition registered for `name`; nothing in the free edition. */
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let { name, props = {} }: { name: SlotName; props?: Record<string, any> } = $props();

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let loaded = $state<Component<any>[]>([]);

  $effect(() => {
    let live = true;
    void Promise.all(slotItems(name).map((load) => load())).then((mods) => {
      if (live) loaded = mods.map((m) => m.default);
    });
    return () => {
      live = false;
    };
  });
</script>

{#each loaded as Item, i (i)}
  <Item {...props} />
{/each}
