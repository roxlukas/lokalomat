/**
 * Markdown for model answers. Model output is untrusted: it is rendered with
 * `marked` and always sanitised with DOMPurify before it reaches the DOM.
 *
 * Math (`$…$`, `$$…$$`, `\(…\)`, `\[…\]`) is typeset with KaTeX (bundled,
 * fonts included). The sanitiser only sees a placeholder for each formula;
 * KaTeX's own output (which needs inline styles) is put in afterwards. KaTeX
 * runs with `trust: false`, so a formula cannot add links or HTML.
 */
import { Marked, type TokenizerAndRendererExtension } from 'marked';
import DOMPurify from 'dompurify';
import katex from 'katex';
import 'katex/dist/katex.min.css';

interface MathToken {
  type: 'mathBlock' | 'mathInline';
  raw: string;
  text: string;
  display: boolean;
}

/** Formulas of the answer being rendered, by placeholder number. */
let formulas: MathToken[] = [];

function placeholder(token: MathToken): string {
  formulas.push(token);
  return `<span data-lkm-math="${formulas.length - 1}"></span>`;
}

const BLOCK = [/^\$\$([\s\S]+?)\$\$[^\S\n]*(?:\n|$)/, /^\\\[([\s\S]+?)\\\][^\S\n]*(?:\n|$)/];
const INLINE: [RegExp, boolean][] = [
  [/^\$\$([\s\S]+?)\$\$/, true],
  [/^\\\[([\s\S]+?)\\\]/, true],
  [/^\\\(([\s\S]+?)\\\)/, false],
  // `$x$`: no space inside the dollars and no digit after, so "$5 and $10" stays text.
  [/^\$(?!\s)((?:\\.|[^\\$\n])+?)(?<!\s)\$(?!\d)/, false],
];

const mathBlock: TokenizerAndRendererExtension = {
  name: 'mathBlock',
  level: 'block',
  start(src) {
    return src.match(/^ {0,3}(?:\$\$|\\\[)/m)?.index;
  },
  tokenizer(src) {
    const trimmed = src.replace(/^ {0,3}/, '');
    for (const re of BLOCK) {
      const m = re.exec(trimmed);
      if (m) return { type: 'mathBlock', raw: src.slice(0, src.length - trimmed.length) + m[0], text: (m[1] ?? '').trim(), display: true };
    }
    return undefined;
  },
  renderer(token) {
    return placeholder(token as unknown as MathToken) + '\n';
  },
};

const mathInline: TokenizerAndRendererExtension = {
  name: 'mathInline',
  level: 'inline',
  start(src) {
    return src.match(/\$|\\[[(]/)?.index;
  },
  tokenizer(src) {
    for (const [re, display] of INLINE) {
      const m = re.exec(src);
      if (m) return { type: 'mathInline', raw: m[0], text: (m[1] ?? '').trim(), display };
    }
    return undefined;
  },
  renderer(token) {
    return placeholder(token as unknown as MathToken);
  },
};

const marked = new Marked({ gfm: true, breaks: true, async: false, extensions: [mathBlock, mathInline] });

function typeset(token: MathToken): string {
  return katex.renderToString(token.text, {
    displayMode: token.display,
    throwOnError: false,
    trust: false,
    strict: 'ignore',
    output: 'htmlAndMathml',
  });
}

export function renderMarkdown(text: string): string {
  formulas = [];
  const html = marked.parse(text) as string;
  const clean = DOMPurify.sanitize(html, {
    USE_PROFILES: { html: true },
    FORBID_TAGS: ['style', 'form', 'input', 'button', 'iframe', 'img'],
    FORBID_ATTR: ['style'],
  });
  const found = formulas;
  formulas = [];
  return clean.replace(/<span data-lkm-math="(\d+)"><\/span>/g, (all, i: string) => {
    const token = found[Number(i)];
    return token ? typeset(token) : '';
  });
}
