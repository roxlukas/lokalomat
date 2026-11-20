<script lang="ts">
  import type { Component, Snippet } from 'svelte';

  let {
    icon: Icon,
    title,
    body,
    note,
    children,
  }: {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    icon: Component<any>;
    title: string;
    body: string;
    note?: string;
    children?: Snippet;
  } = $props();
</script>

<section class="empty">
  <div class="icon"><Icon size={28} aria-hidden="true" /></div>
  <h1>{title}</h1>
  <p class="body">{body}</p>
  {#if note}<p class="note-chip">{note}</p>{/if}
  {#if children}{@render children()}{/if}
</section>

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-3);
    max-width: 520px;
    margin: auto;
    padding: var(--space-10) var(--space-4);
    text-align: center;
  }

  .icon {
    display: grid;
    place-items: center;
    width: 56px;
    height: 56px;
    border-radius: var(--radius-l);
    background: var(--surface-2);
    color: var(--text-muted);
  }

  h1 {
    font-size: var(--text-xl);
  }

  .body {
    color: var(--text-muted);
  }

  .note-chip {
    padding: var(--space-1) var(--space-3);
    border-radius: 999px;
    background: var(--info-bg);
    color: var(--info);
    font-size: var(--text-s);
  }
</style>
