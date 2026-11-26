import { describe, expect, it } from 'vitest';
import { MAX_MAILTO_LENGTH, REPORT_EMAIL, answerReportMailto, reportBody, reportMailto, reportSubject } from './report';

const base = { description: ' Nie działa czat ', edition: 'free' as const, version: '0.2.1', attachSystem: true, system: 'windows/x86_64' };

describe('report a problem', () => {
  it('names the edition in the subject', () => {
    expect(reportSubject('free', '0.2.1')).toBe('[Lokalomat 0.2.1] Problem report');
    expect(reportSubject('pro', '0.2.1')).toBe('[Lokalomat Pro 0.2.1] Problem report');
  });

  it('adds system info only when allowed', () => {
    expect(reportBody(base)).toContain('windows/x86_64');
    expect(reportBody({ ...base, attachSystem: false })).not.toContain('windows');
    expect(reportBody({ ...base, reply: 'a@b.pl' })).toContain('Reply to: a@b.pl');
  });

  it('builds a mailto link to the support address', () => {
    const { url, truncated } = reportMailto(base);
    expect(url.startsWith(`mailto:${REPORT_EMAIL}?subject=`)).toBe(true);
    expect(truncated).toBe(false);
  });

  it('shortens very long texts', () => {
    const { url, truncated } = reportMailto({ ...base, description: 'ą'.repeat(5000) });
    expect(truncated).toBe(true);
    expect(url.length).toBeLessThanOrEqual(MAX_MAILTO_LENGTH);
  });
});

describe('reporting an AI answer', () => {
  it('puts the answer, the model and where it ran into the mail', () => {
    const { url, truncated } = answerReportMailto({ answer: 'Zła odpowiedź', model: 'Gemma', where: 'phone', version: '0.2.7' }, 'Why:');
    expect(truncated).toBe(false);
    expect(url.startsWith(`mailto:${REPORT_EMAIL}?subject=`)).toBe(true);
    const body = decodeURIComponent(url.split('&body=')[1] ?? '');
    expect(body).toContain('Zła odpowiedź');
    expect(body).toContain('Model: Gemma');
    expect(body).toContain('Where: phone');
  });

  it('shortens a long answer to fit a mailto link', () => {
    const { url, truncated } = answerReportMailto({ answer: 'ą'.repeat(5000), where: 'phone' }, 'Why:');
    expect(truncated).toBe(true);
    expect(url.length).toBeLessThanOrEqual(MAX_MAILTO_LENGTH + 20);
  });
});
