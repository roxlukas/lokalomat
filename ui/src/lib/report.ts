// "Report a problem": the message is prepared locally and handed to the user's
// mail program (mailto:), so nothing is sent without the user pressing Send there.

export const REPORT_EMAIL = 'lokalomat@aitransformation.pl';

/** Mail programs and browsers cut very long mailto links; keep well under the limit. */
export const MAX_MAILTO_LENGTH = 1800;

export interface ReportInput {
  description: string;
  reply?: string;
  edition: 'free' | 'pro';
  version: string;
  system?: string;
  attachSystem: boolean;
}

export function reportSubject(edition: 'free' | 'pro', version: string): string {
  return `[Lokalomat ${edition === 'pro' ? 'Pro ' : ''}${version}] Problem report`;
}

export function reportBody(i: ReportInput): string {
  const lines = [i.description.trim(), ''];
  if (i.reply?.trim()) lines.push(`Reply to: ${i.reply.trim()}`);
  if (i.attachSystem) {
    lines.push('--', `Lokalomat ${i.edition === 'pro' ? 'Pro ' : ''}${i.version}`);
    if (i.system) lines.push(i.system);
  }
  return lines.join('\n');
}

/** The mailto link and whether the text had to be shortened to fit. */
export function reportMailto(i: ReportInput): { url: string; truncated: boolean } {
  const subject = reportSubject(i.edition, i.version);
  const build = (body: string) =>
    `mailto:${REPORT_EMAIL}?subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(body)}`;
  let body = reportBody(i);
  let url = build(body);
  if (url.length <= MAX_MAILTO_LENGTH) return { url, truncated: false };
  while (url.length > MAX_MAILTO_LENGTH && body.length > 0) {
    body = body.slice(0, Math.max(0, body.length - Math.ceil((url.length - MAX_MAILTO_LENGTH) / 3) - 1));
    url = build(body);
  }
  return { url, truncated: true };
}

/** "Zgłoś" on an AI answer (Google Play's AI-generated content policy, ADR 0040). */
export interface AnswerReport {
  answer: string;
  /** The model that wrote it, when known. */
  model?: string;
  /** "phone" or the computer's name. */
  where: string;
  version?: string;
}

export function answerReportSubject(version?: string): string {
  return `[Lokalomat${version ? ` ${version}` : ''}] AI answer report`;
}

export function answerReportBody(r: AnswerReport, reasonLine: string): string {
  const meta = [`Where: ${r.where}`, r.model ? `Model: ${r.model}` : '', r.version ? `Lokalomat ${r.version}` : ''].filter(Boolean);
  return [reasonLine, '', '', '--', ...meta, '--', r.answer.trim()].join('\n');
}

/** The mailto link of an answer report; a long answer is shortened from its end. */
export function answerReportMailto(r: AnswerReport, reasonLine: string): { url: string; truncated: boolean } {
  const subject = answerReportSubject(r.version);
  const build = (answer: string) =>
    `mailto:${REPORT_EMAIL}?subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(answerReportBody({ ...r, answer }, reasonLine))}`;
  let answer = r.answer.trim();
  let url = build(answer);
  if (url.length <= MAX_MAILTO_LENGTH) return { url, truncated: false };
  while (url.length > MAX_MAILTO_LENGTH && answer.length > 0) {
    answer = answer.slice(0, Math.max(0, answer.length - Math.ceil((url.length - MAX_MAILTO_LENGTH) / 3) - 1));
    url = build(`${answer}…`);
  }
  return { url: build(`${answer}…`), truncated: true };
}
