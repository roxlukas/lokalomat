import { describe, expect, it } from 'vitest';
import type { DecisionQuestion, Job } from './ipc';
import {
  canAsk,
  cleanQuestions,
  gateFits,
  jobDecision,
  labelsOf,
  newQuestion,
  questionIssue,
  withType,
} from './decisions';

const choice: DecisionQuestion = { id: 'q1', instructions: 'Który dział?', type: 'choice', options: ['karty', 'kredyty'] };

describe('decision questions', () => {
  it('gets fresh ids and keeps options when the type changes', () => {
    const q = newQuestion('choice', [choice]);
    expect(q.id).toBe('q2');
    expect(q.type === 'choice' && q.options).toEqual(['', '']);
    const score = withType(choice, 'score');
    expect(score.type === 'score' && score.levels).toEqual(['karty', 'kredyty']);
    expect(withType(choice, 'yes_no')).toEqual({ id: 'q1', instructions: 'Który dział?', type: 'yes_no' });
  });

  it('reports what is missing', () => {
    expect(questionIssue(choice)).toBeNull();
    expect(questionIssue({ ...choice, instructions: ' ' })).toBe('no_question');
    expect(questionIssue({ ...choice, options: ['a'] })).toBe('options');
    expect(questionIssue({ ...choice, options: ['a', ''] })).toBe('options');
    expect(questionIssue({ ...choice, options: ['a', 'a '] })).toBe('duplicate');
    expect(questionIssue({ id: 'q', instructions: 'Spam?', type: 'yes_no' })).toBeNull();
    expect(canAsk('tekst', [choice])).toBe(true);
    expect(canAsk(' ', [choice])).toBe(false);
    expect(canAsk('tekst', [])).toBe(false);
  });

  it('trims what is sent and checks gates', () => {
    const [c] = cleanQuestions([{ ...choice, instructions: ' Który? ', options: [' a ', 'b'] }]);
    expect(c).toEqual({ id: 'q1', instructions: 'Który?', type: 'choice', options: ['a', 'b'] });
    const yn: DecisionQuestion = { id: 'q', instructions: 'Spam?', type: 'yes_no' };
    expect(labelsOf(yn)).toEqual(['yes', 'no']);
    expect(gateFits({ answer: 'no' }, yn)).toBe(true);
    expect(gateFits({ answer: 'maybe' }, yn)).toBe(false);
    expect(gateFits({ answer: 'yes', min_percent: 120 }, yn)).toBe(false);
  });

  it('reads a decision job', () => {
    const job = {
      params: { questions: [choice] },
      outputs: { answers: [{ id: 'q1', label: 'karty', probabilities: [{ label: 'karty', p: 0.9 }], confidence: 0.8 }] },
    } as unknown as Job;
    expect(jobDecision(job).answers[0]!.label).toBe('karty');
    expect(jobDecision({ params: {}, outputs: {} } as unknown as Job)).toEqual({ questions: [], answers: [] });
  });
});
