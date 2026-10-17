import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import { ModelCard } from './ModelCard';

const render = (markdown: string) =>
  renderToStaticMarkup(createElement(ModelCard, { markdown, repo: 'unsloth/Qwen3-8B-GGUF' }));

describe('ModelCard', () => {
  it('keeps the card’s pictures and layout', () => {
    const html = render(
      '<div align="center"><img src="images/logo.png" width="60%" alt="Logo"></div>\n\n' +
        '![Bench](https://huggingface.co/Qwen/Qwen3-8B/blob/main/bench.png)\n\n' +
        '<details><summary>More</summary>Hidden text</details>\n\n| a | b |\n|---|---|\n| 1 | 2 |',
    );
    expect(html).toContain('align="center"');
    expect(html).toContain('src="https://huggingface.co/unsloth/Qwen3-8B-GGUF/resolve/main/images/logo.png"');
    expect(html).toContain('width="60%"');
    expect(html).toContain('src="https://huggingface.co/Qwen/Qwen3-8B/resolve/main/bench.png"');
    expect(html).toContain('<details><summary>More</summary>');
    expect(html).toContain('<table>');
  });

  it('draws GitHub callouts as notes', () => {
    const html = render('> [!NOTE]\n> These are the weights for Transformers.');
    expect(html).toContain('role="note"');
    expect(html).toContain('>Note<');
    expect(html).not.toContain('[!NOTE]');
    expect(html).toContain('These are the weights for Transformers.');
    expect(render('> Just a quote')).toContain('<blockquote>');
  });

  it('drops anything that could run or restyle the app', () => {
    const html = render(
      '<script>window.__x = 1</script>\n\n' +
        '<img src="a.png" onerror="alert(1)">\n\n' +
        '<div style="position:fixed;inset:0">Cover</div>\n\n' +
        '<a href="javascript:alert(1)">Click</a>\n\n' +
        '<iframe src="https://evil.example"></iframe>\n\n' +
        '<style>body{display:none}</style>\n\n' +
        '<form action="https://evil.example"><input name="token"></form>',
    );
    expect(html).not.toContain('<script');
    expect(html).not.toContain('__x');
    expect(html).not.toContain('onerror');
    expect(html).not.toContain('style=');
    expect(html).not.toContain('javascript:');
    expect(html).not.toContain('<iframe');
    expect(html).not.toContain('<style');
    expect(html).not.toContain('<form');
    expect(html).toContain('Cover');
  });
});
