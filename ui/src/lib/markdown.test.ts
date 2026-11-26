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

  it('typesets inline and display math', () => {
    const inline = renderMarkdown('Pole koła to $\\pi r^2$, a \\(e^{i\\pi}\\) też.');
    expect(inline.match(/class="katex"/g)).toHaveLength(2);
    expect(inline).toContain('<annotation encoding="application/x-tex">\\pi r^2</annotation>');
    expect(inline).not.toContain('katex-display');

    const display = renderMarkdown('Wynik:\n\n\\[\n\\boxed{391}\n\\]\n\nKoniec.\n\n$$a^2+b^2=c^2$$');
    expect(display.match(/katex-display/g)).toHaveLength(2);
    expect(display).toContain('Koniec.');
    expect(display).not.toContain('\\[');
  });

  it('leaves prices and code alone', () => {
    const html = renderMarkdown('Kosztuje $5, a z dostawą $10.\n\n`$x$` i\n\n```\n$$y$$\n```');
    expect(html).not.toContain('katex');
    expect(html).toContain('$5, a z dostawą $10.');
    expect(html).toContain('<code>$x$</code>');
  });

  it('keeps formulas from adding links or HTML', () => {
    const html = renderMarkdown('$\\href{javascript:alert(1)}{x}$ $\\htmlClass{evil}{y}$ <span data-lkm-math="9"></span>');
    // KaTeX rejects them (shown in red as text); the TeX source only stays in the annotation.
    expect(html).not.toMatch(/<a[\s>]|href=|class="[^"]*evil/i);
    expect(html).not.toContain('data-lkm-math');
  });

  it('shows a broken formula instead of failing', () => {
    expect(() => renderMarkdown('$\\frac{1}{$')).not.toThrow();
  });
});
