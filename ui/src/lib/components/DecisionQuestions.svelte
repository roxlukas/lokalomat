<script lang="ts">
  import Plus from '@lucide/svelte/icons/plus';
  import X from '@lucide/svelte/icons/x';
  import { t } from '../i18n/index.svelte';
  import type { DecisionQuestion } from '../ipc';
  import {
    MAX_FIELD_CHARS,
    MAX_QUESTIONS,
    QUESTION_TYPES,
    itemsOf,
    maxItems,
    newQuestion,
    questionIssue,
    withType,
    type QuestionType,
  } from '../decisions';

  /**
   * Editor of decision questions (Workshop "Decyzje"; one question in a
   * pipeline step). Every change assigns a new array, so function bindings work.
   */
  let { questions = $bindable(), single = false }: { questions: DecisionQuestion[]; single?: boolean } = $props();

  function setQuestion(i: number, q: DecisionQuestion) {
    questions = questions.map((x, k) => (k === i ? q : x));
  }

  function setItems(i: number, items: string[]) {
    const q = questions[i]!;
    if (q.type === 'choice') setQuestion(i, { ...q, options: items });
    else if (q.type === 'score') setQuestion(i, { ...q, levels: items });
  }

  function setType(i: number, type: QuestionType) {
    setQuestion(i, withType(questions[i]!, type));
  }

  function setItem(i: number, j: number, value: string) {
    setItems(i, itemsOf(questions[i]!).map((o, k) => (k === j ? value : o)));
  }

  function addItem(i: number) {
    const q = questions[i]!;
    if (itemsOf(q).length < maxItems(q)) setItems(i, [...itemsOf(q), '']);
  }

  function removeItem(i: number, j: number) {
    const items = itemsOf(questions[i]!);
    if (items.length > 2) setItems(i, items.filter((_, k) => k !== j));
  }

  function addQuestion() {
    if (questions.length < MAX_QUESTIONS) questions = [...questions, newQuestion('choice', questions)];
  }
</script>

<div class="questions">
  {#each questions as q, i (q.id)}
    {@const issue = questionIssue(q)}
    <fieldset class="question">
      <legend class="sr-only">{t('decisions.questionN', { n: i + 1 })}</legend>
      <div class="head">
        <div class="chip-group" role="radiogroup" aria-label={t('decisions.type')}>
          {#each QUESTION_TYPES as type (type)}
            <button type="button" role="radio" aria-checked={q.type === type} class="chip" class:active={q.type === type}
              title={t(`decisions.typeHints.${type}`)} onclick={() => setType(i, type)}>{t(`decisions.types.${type}`)}</button>
          {/each}
        </div>
        {#if !single && questions.length > 1}
          <button type="button" class="icon-btn" aria-label={t('decisions.removeQuestion')} title={t('decisions.removeQuestion')}
            onclick={() => (questions = questions.filter((_, k) => k !== i))}><X size={14} aria-hidden="true" /></button>
        {/if}
      </div>
      <label class="field">
        <span class="sr-only">{t('decisions.question')}</span>
        <input type="text" value={q.instructions} maxlength={MAX_FIELD_CHARS} placeholder={t(`decisions.placeholders.${q.type}`)}
          oninput={(e) => setQuestion(i, { ...q, instructions: e.currentTarget.value })} />
      </label>
      {#if q.type !== 'yes_no'}
        {@const items = itemsOf(q)}
        <ol class="items" aria-label={q.type === 'score' ? t('decisions.levels') : t('decisions.options')}>
          {#each items as item, j (j)}
            <li>
              <input type="text" value={item} maxlength={MAX_FIELD_CHARS} oninput={(e) => setItem(i, j, e.currentTarget.value)}
                aria-label={q.type === 'score' ? t('decisions.levelN', { n: j + 1 }) : t('decisions.optionN', { n: j + 1 })}
                placeholder={q.type === 'score' ? (j === 0 ? t('decisions.lowest') : j === items.length - 1 ? t('decisions.highest') : '') : ''} />
              {#if items.length > 2}
                <button type="button" class="icon-btn" aria-label={t('decisions.removeItem')} onclick={() => removeItem(i, j)}><X size={12} aria-hidden="true" /></button>
              {/if}
            </li>
          {/each}
        </ol>
        {#if items.length < maxItems(q)}
          <button type="button" class="link small" onclick={() => addItem(i)}>
            <Plus size={12} aria-hidden="true" />{q.type === 'score' ? t('decisions.addLevel') : t('decisions.addOption')}
          </button>
        {/if}
      {/if}
      {#if issue && q.instructions.trim()}<p class="muted small">{t(`decisions.issues.${issue}`)}</p>{/if}
    </fieldset>
  {/each}
  {#if !single && questions.length < MAX_QUESTIONS}
    <button type="button" class="link small add" onclick={addQuestion}><Plus size={12} aria-hidden="true" />{t('decisions.addQuestion')}</button>
  {/if}
</div>

<style>
  .questions {
    display: grid;
    gap: var(--space-2);
    max-height: 40vh;
    overflow-y: auto;
  }

  .question {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .chip-group {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
  }

  .chip {
    padding: 2px var(--space-3);
    border: 1px solid transparent;
    border-radius: var(--radius-pill);
    background: transparent;
    color: var(--text-muted);
    font-size: var(--text-s);
  }

  .chip.active {
    background: var(--surface-2);
    border-color: var(--border-strong);
    color: var(--text);
  }

  .field input,
  .items input {
    width: 100%;
    min-height: 30px;
    font-size: var(--text-s);
  }

  .items {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: var(--space-1) var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .items li {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .icon-btn {
    display: inline-flex;
    padding: 4px;
    border: 0;
    border-radius: var(--radius-s);
    background: transparent;
    color: var(--text-muted);
  }

  .icon-btn:hover {
    background: var(--surface-2);
  }

  .link {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    justify-self: start;
    padding: 0;
    border: 0;
    background: none;
    color: var(--focus);
  }

  .small {
    font-size: var(--text-s);
  }
</style>
