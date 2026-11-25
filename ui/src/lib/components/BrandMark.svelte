<script lang="ts">
  // The Lokalomat mark: purple (epic) in the free edition, gold (legendary) in Pro (desktop header, phone).
  // Two gradients per mark; ids are unique so several marks can share a page.
  let { size = 24, pro = false }: { size?: number; pro?: boolean } = $props();

  const id = `lm${Math.random().toString(36).slice(2, 8)}`;
  const palette = $derived(
    pro
      ? { l1: '#d9bf6f', l2: '#85692c', d1: '#3b4a7a', d2: '#151c3b' }
      : { l1: '#d8b4fe', l2: '#9333ea', d1: '#6d28d9', d2: '#2e1065' },
  );
</script>

<svg width={size} height={size} viewBox="150 30 365 325" aria-hidden="true">
  <defs>
    <linearGradient id="{id}a" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color={palette.l1} />
      <stop offset="1" stop-color={palette.l2} />
    </linearGradient>
    <linearGradient id="{id}b" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color={palette.d1} />
      <stop offset="1" stop-color={palette.d2} />
    </linearGradient>
  </defs>
  <path d="M245 40H340L255 345H160Z" fill="url(#{id}b)" />
  <path d="M290 285H505L460 345H272Z" fill="url(#{id}a)" />
  <path d="M392 285L430 232H505V285Z" fill="url(#{id}a)" opacity=".55" />
</svg>
