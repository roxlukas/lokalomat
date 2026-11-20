<script lang="ts">
  import { t } from '../lib/i18n/index.svelte';
  import { readLocal, writeLocal } from '../lib/storage';
  import { api, type Mode, type ModelOption } from './api';

  /**
   * Which installed model of the computer runs this mode. The choice is kept
   * on the phone; "" means the computer's default model.
   */
  let { mode, value = $bindable('') }: { mode: Mode; value?: string } = $props();

  let options = $state<ModelOption[]>([]);
  const key = () => `lokalsi.mobile.model.${mode}`;

  $effect(() => {
    const m = mode;
    value = readLocal<string>(key(), '');
    api
      .models(m)
      .then((list) => {
        options = list;
        // A model removed on the computer falls back to the default.
        if (value && !list.some((o) => o.id === value)) value = '';
      })
      .catch(() => (options = []));
  });

  function pick(id: string) {
    value = id;
    writeLocal(key(), id);
  }

  const fallback = $derived(options.find((o) => o.default)?.name ?? '');
</script>

{#if options.length > 1}
  <label class="picker">
    <span class="small muted">{t('mobile.model.label')}</span>
    <select {value} onchange={(e) => pick(e.currentTarget.value)}>
      <option value="">{fallback ? t('mobile.model.default', { name: fallback }) : t('mobile.model.auto')}</option>
      {#each options as o (o.id)}<option value={o.id}>{o.name}</option>{/each}
    </select>
  </label>
{:else if options[0]}
  <p class="small muted one">{t('mobile.model.only', { name: options[0].name })}</p>
{/if}

<style>
  .picker {
    display: grid;
    gap: var(--space-1);
  }

  .picker select {
    width: 100%;
  }

  .small {
    font-size: var(--text-s);
  }

  .muted {
    color: var(--text-muted);
  }

  .one {
    margin: 0;
  }
</style>
