<script lang="ts">
  import { onMount } from 'svelte';
  import Send from '@lucide/svelte/icons/send';
  import ChatMessage from '../../lib/components/ChatMessage.svelte';
  import SlotHost from '../../lib/components/SlotHost.svelte';
  import { chat, chatModelOptions, engines } from '../../lib/chat.svelte';
  import { describeError } from '../../lib/feedback';
  import { t } from '../../lib/i18n/index.svelte';
  import { router } from '../../lib/router.svelte';
  import { models } from '../../lib/stores.svelte';
  import { workshop } from '../../lib/workshop.svelte';

  /** The chat mode: messages in the output area, model choice in the top bar. */
  let options = $derived(chatModelOptions(models.list));
  let selected = $derived(
    options.find((o) => o.modelId === chat.model?.modelId && o.variantId === chat.model?.variantId) ?? null,
  );
  // Plug and play: a missing engine installs itself, chat only needs a model.
  let engineInstall = $derived(engines.llama?.state === 'installing' ? engines.llama.install_progress : null);
  let canSend = $derived(options.length > 0 && workshop.prompt.trim().length > 0 && !chat.active);

  async function send() {
    const text = workshop.prompt;
    workshop.prompt = '';
    workshop.stick = true;
    try {
      await chat.send(text);
    } catch (err) {
      workshop.prompt = text;
      workshop.error = describeError(err);
    }
  }

  function chooseModel(value: string) {
    const o = options.find((x) => `${x.modelId}/${x.variantId}` === value);
    chat.setModel(o ? { modelId: o.modelId, variantId: o.variantId } : null);
  }

  onMount(() => {
    workshop.attach('chat', {
      output,
      top,
      controls,
      get hasOutput() {
        return chat.messages.length > 0;
      },
      get scrollKey() {
        return `${chat.messages.length}:${chat.messages.at(-1)?.content.length ?? 0}`;
      },
      get canSend() {
        return canSend;
      },
      send,
      get placeholder() {
        return t('workshop.inputPlaceholder');
      },
      get submitLabel() {
        return t('workshop.send');
      },
      submitIcon: Send,
      get stop() {
        const active = chat.active;
        return active ? { label: t('chat.stop'), run: () => chat.cancel(active.id) } : undefined;
      },
    });
    return () => workshop.detach('chat');
  });
</script>

{#snippet output()}
  <div class="messages" aria-live="polite">
    {#each chat.messages as m (m.id)}
      <ChatMessage message={m} onCancel={(id) => chat.cancel(id)} />
    {/each}
  </div>
{/snippet}

{#snippet top()}
  <!-- Pro adds the "tools on" chip here (MCP programs). -->
  <SlotHost name="chat-bar" props={{ model: selected ?? options[0] }} />
  {#if options.length}
    <label class="model">
      <span class="sr-only">{t('chat.model')}</span>
      <select value={selected ? `${selected.modelId}/${selected.variantId}` : ''} onchange={(e) => chooseModel(e.currentTarget.value)}>
        <option value="">{t('chat.defaultModel')}</option>
        {#each options as o (`${o.modelId}/${o.variantId}`)}
          <option value={`${o.modelId}/${o.variantId}`}>{o.label}</option>
        {/each}
      </select>
    </label>
  {/if}
{/snippet}

{#snippet controls()}
  {#if engineInstall}
    <p class="note note-info setup" role="status">
      {t('chat.preparingEngine', { percent: engineInstall[1] ? Math.round((engineInstall[0] / engineInstall[1]) * 100) : 0 })}
    </p>
  {/if}
  {#if options.length === 0}
    <p class="note note-info setup">
      {t('chat.needModel')}
      <button type="button" class="link" onclick={() => router.goModels('llm')}>{t('chat.goModels')}</button>
    </p>
  {/if}
{/snippet}
