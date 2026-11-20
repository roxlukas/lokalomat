// @vitest-environment jsdom
import { describe, expect, it } from 'vitest';
import { renderMarkdown } from './markdown';

describe('renderMarkdown', () => {
  it('renders markdown', () => {
    const html = renderMarkdown('**gruby** i `kod`\n\n```rs\nfn main() {}\n```');
    expect(html).toContain('<strong>gruby</strong>');
    expect(html).toContain('<code>kod</code>');
    expect(html).toContain('<pre>');
  });

  it('strips scripts, handlers and images from model output', () => {
    const html = renderMarkdown('<script>alert(1)</script><a href="javascript:alert(1)" onclick="x()">x</a><img src=x onerror=alert(1)>');
    expect(html).not.toMatch(/<script|onclick|onerror|javascript:|<img/i);
  });
});
