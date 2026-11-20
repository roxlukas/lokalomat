/**
 * Decision questions (ADR 0024): the Workshop "Decyzje" form and the pipeline
 * Decide step. Mirrors `lokalsi_core::decisions` (the backend validates again).
 */
import type { DecisionAnswer, DecisionGate, DecisionQuestion, Job } from './ipc';

export type QuestionType = DecisionQuestion['type'];
export const QUESTION_TYPES: QuestionType[] = ['choice', 'yes_no', 'score'];
/** llama.cpp reads the options of the Basal files as the codes A–Z. */
export const MAX_OPTIONS = 26;
export const MAX_LEVELS = 10;
export const MAX_QUESTIONS = 12;
export const MAX_STATE_CHARS = 20_000;
export const MAX_FIELD_CHARS = 500;
export const YES = 'yes';
export const NO = 'no';

/** A new question of `type` with id `q<n>` not used in `existing`. */
export function newQuestion(type: QuestionType, existing: DecisionQuestion[] = []): DecisionQuestion {
  let n = existing.length + 1;
  while (existing.some((q) => q.id === `q${n}`)) n++;
  return withType({ id: `q${n}`, instructions: '', type: 'yes_no' }, type);
}

/** The same question as another type; options and levels are kept when they fit. */
export function withType(q: DecisionQuestion, type: QuestionType): DecisionQuestion {
  const items = q.type === 'choice' ? q.options : q.type === 'score' ? q.levels : [];
  const base = { id: q.id, instructions: q.instructions };
  if (type === 'choice') return { ...base, type, options: items.length >= 2 ? [...items] : ['', ''] };
  if (type === 'score') return { ...base, type, levels: items.length >= 2 ? items.slice(0, MAX_LEVELS) : ['', '', ''] };
  return { ...base, type };
}

/** Options of a choice, levels of a score; empty for yes/no. */
export function itemsOf(q: DecisionQuestion): string[] {
  return q.type === 'choice' ? q.options : q.type === 'score' ? q.levels : [];
}

/** Answer labels a question can give, in order (yes/no → `yes`, `no`). */
export function labelsOf(q: DecisionQuestion): string[] {
  return q.type === 'yes_no' ? [YES, NO] : itemsOf(q);
}

export function maxItems(q: DecisionQuestion): number {
  return q.type === 'score' ? MAX_LEVELS : MAX_OPTIONS;
}

/** Why a question cannot be asked yet (`decisions.issues.*`), or null. */
export function questionIssue(q: DecisionQuestion): string | null {
  if (!q.instructions.trim()) return 'no_question';
  if (q.instructions.length > MAX_FIELD_CHARS) return 'too_long';
  if (q.type === 'yes_no') return null;
  const items = itemsOf(q).map((o) => o.trim());
  if (items.length < 2 || items.some((o) => !o)) return 'options';
  if (new Set(items).size !== items.length) return 'duplicate';
  if (items.length > maxItems(q)) return 'options';
  if (items.some((o) => o.length > MAX_FIELD_CHARS)) return 'too_long';
  return null;
}

/** Questions ready to send: options trimmed, ids unique. */
export function cleanQuestions(questions: DecisionQuestion[]): DecisionQuestion[] {
  return questions.map((q) => {
    const instructions = q.instructions.trim();
    if (q.type === 'choice') return { ...q, instructions, options: q.options.map((o) => o.trim()) };
    if (q.type === 'score') return { ...q, instructions, levels: q.levels.map((o) => o.trim()) };
    return { ...q, instructions };
  });
}

export function canAsk(state: string, questions: DecisionQuestion[]): boolean {
  return (
    state.trim().length > 0 &&
    state.length <= MAX_STATE_CHARS &&
    questions.length > 0 &&
    questions.length <= MAX_QUESTIONS &&
    questions.every((q) => questionIssue(q) === null)
  );
}

/** True when `gate` names an answer the question can give. */
export function gateFits(gate: DecisionGate, q: DecisionQuestion): boolean {
  const t = gate.min_percent;
  return labelsOf(q).includes(gate.answer) && (t == null || (t >= 0 && t <= 100));
}

/** Questions and answers of a decision job (answers once it is done). */
export function jobDecision(job: Job): { questions: DecisionQuestion[]; answers: DecisionAnswer[] } {
  const questions = Array.isArray(job.params['questions']) ? (job.params['questions'] as DecisionQuestion[]) : [];
  const answers = Array.isArray(job.outputs['answers']) ? (job.outputs['answers'] as DecisionAnswer[]) : [];
  return { questions, answers };
}

/** Starter questions for an empty form. */
export function starterQuestions(lang: 'pl' | 'en'): DecisionQuestion[] {
  return lang === 'en'
    ? [{ id: 'q1', instructions: 'Is this message spam?', type: 'yes_no' }]
    : [{ id: 'q1', instructions: 'Czy ta wiadomość to spam?', type: 'yes_no' }];
}
