<script lang="ts">
  import { onMount } from 'svelte';
  import Scale from '@lucide/svelte/icons/scale';
  import DecisionCard from '../../lib/components/DecisionCard.svelte';
  import DecisionQuestions from '../../lib/components/DecisionQuestions.svelte';
  import ModeNotes from '../../lib/components/ModeNotes.svelte';
  import { canAsk, cleanQuestions, MAX_STATE_CHARS, starterQuestions } from '../../lib/decisions';
  import { describeError } from '../../lib/feedback';
  import { t } from '../../lib/i18n/index.svelte';
  import type { DecisionQuestion } from '../../lib/ipc';
  import { jobs } from '../../lib/jobs.svelte';
  import { modelOptions } from '../../lib/media';
  import { settings } from '../../lib/settings.svelte';
  import { models } from '../../lib/stores.svelte';
  import { readLocal, writeLocal } from '../../lib/storage';
  import { engineStatus, workshop } from '../../lib/workshop.svelte';

  /** The decision mode (Expert): typed answers about a text from a decision model. */
  let decisionModel = $state<string>(readLocal<string>('lokalsi.decision-model', ''));
  let questions = $state<DecisionQuestion[]>(
    readLocal<DecisionQuestion[] | null>('lokalsi.decision-questions', null) ?? starterQuestions(settings.value.language === 'en' ? 'en' : 'pl'),
  );
  let options = $derived(modelOptions(models.list, 'decision'));
  let status = $derived(engineStatus('llama-cpp'));
  let modeJobs = $derived(jobs.of('decision'));
  let canSend = $derived(options.length > 0 && canAsk(workshop.prompt, questions) && !status.unavailable);

  $effect(() => writeLocal('lokalsi.decision-model', decisionModel));
  $effect(() => writeLocal('lokalsi.decision-questions', $state.snapshot(questions)));

  function chosen(value: string) {
    const [modelId, variantId] = value.split('/');
    return modelId && variantId ? { modelId, variantId } : undefined;
  }

  /** "Użyj ponownie" on a decision card: the same text and questions. */
  function reuseDecision(state: string, qs: DecisionQuestion[]) {
    workshop.prompt = state;
    if (qs.length) questions = structuredClone(qs);
    workshop.focusInput();
  }

  async function decide() {
    workshop.stick = true;
    try {
      await jobs.decide({ state: workshop.prompt.trim(), questions: cleanQuestions(questions), ...(chosen(decisionModel) ?? {}) });
      // The text stays, so other questions can be asked about it.
    } catch (err) {
      workshop.error = describeError(err);
    }
  }

  onMount(() => {
    workshop.attach('decision', {
      output,
      controls,
      get hasOutput() {
        return modeJobs.length > 0;
      },
      get scrollKey() {
        return `${modeJobs.length}:${modeJobs.at(-1)?.status ?? ''}`;
      },
      get canSend() {
        return canSend;
      },
      send: decide,
      get placeholder() {
        return t('workshop.decision.placeholder');
      },
      maxlength: MAX_STATE_CHARS,
      get submitLabel() {
        return t('workshop.decision.ask');
      },
      submitIcon: Scale,
    });
    return () => workshop.detach('decision');
  });
</script>

{#snippet output()}
  <div class="messages" aria-live="polite">
    {#each modeJobs as j (j.id)}
      <DecisionCard job={j} onReuse={reuseDecision} />
    {/each}
  </div>
{/snippet}

{#snippet controls()}
  <div class="chips">
    {#if options.length > 1}
      <label class="model">
        <span class="sr-only">{t('chat.model')}</span>
        <select bind:value={decisionModel}>
          <option value="">{t('chat.defaultModel')}</option>
          {#each options as o (`${o.modelId}/${o.variantId}`)}
            <option value={`${o.modelId}/${o.variantId}`}>{o.label}</option>
          {/each}
        </select>
      </label>
    {/if}
    <span class="muted small">{t('workshop.decision.note')}</span>
  </div>
  <DecisionQuestions bind:questions />
  <ModeNotes mode="decision" {status} optionCount={options.length} />
{/snippet}
