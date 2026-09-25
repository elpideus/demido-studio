import { describe, expect, it } from 'vitest';

import { prepareMath } from './mathText';

describe('prepareMath', () => {
  it('escapes prices so they are not read as math', () => {
    expect(prepareMath('falling from $4,416.68 to $4,270.90.')).toBe('falling from \\$4,416.68 to \\$4,270.90.');
    expect(prepareMath('a **$462 range** between $4,697 and $4,235')).toBe(
      'a **\\$462 range** between \\$4,697 and \\$4,235',
    );
    expect(prepareMath('costs $5 and $10 more')).toBe('costs \\$5 and \\$10 more');
    expect(prepareMath('$5 ($2 off), $5/$10 or ~$4.8B')).toBe('\\$5 (\\$2 off), \\$5/\\$10 or ~\\$4.8B');
  });

  it('keeps formulas that start with a number', () => {
    expect(prepareMath('$2^{10}$ is 1024')).toBe('$2^{10}$ is 1024');
    expect(prepareMath('$x_1 + 5$')).toBe('$x_1 + 5$');
  });

  it('keeps a formula that starts with a number and closes on the same line', () => {
    expect(prepareMath('$2x + 1$ and $3y$')).toBe('$2x + 1$ and $3y$');
    expect(prepareMath('$2x + 1$ costs $5')).toBe('$2x + 1$ costs \\$5');
  });

  it('does not take the next price, or a `$` on another line, as the end of a formula', () => {
    expect(prepareMath('costs $5; see $x$ below')).toBe('costs \\$5; see $x$ below');
    expect(prepareMath('$5 today\nand x$ tomorrow')).toBe('\\$5 today\nand x$ tomorrow');
    expect(prepareMath('costs $5 (was \\$6)')).toBe('costs \\$5 (was \\$6)');
    expect(prepareMath('$5 and $$ x')).toBe('\\$5 and $$ x');
  });

  it('does not take the `$` of a currency code as the end of a formula', () => {
    expect(prepareMath('Gold closed at $4,416 per ounce (in US$).')).toBe(
      'Gold closed at \\$4,416 per ounce (in US$).',
    );
    expect(prepareMath('It opened at $14, about 70 R$ per share.')).toBe('It opened at \\$14, about 70 R$ per share.');
    expect(prepareMath('Target $5 (US$ 5.20 in Canada)')).toBe('Target \\$5 (US$ 5.20 in Canada)');
    for (const code of ['A', 'C', 'HK', 'NZ', 'S', 'NT', 'MX']) {
      expect(prepareMath(`from $5 to 7 ${code}$ each`)).toBe(`from \\$5 to 7 ${code}$ each`);
    }
  });

  it('still closes a formula on a capital that does not start a currency code', () => {
    expect(prepareMath('$2X$ and $2x + 1$')).toBe('$2X$ and $2x + 1$');
    expect(prepareMath('$2 + ABCD$')).toBe('$2 + ABCD$');
    for (const formula of ['$1 - F_X$', '$2P_A$', '$0.5 m_A$', '$1 + r_B$', '$1 + e^A$']) {
      expect(prepareMath(formula)).toBe(formula);
    }
    // The accepted trade-off: a lone capital after a space reads as a currency code.
    expect(prepareMath('$2 + A$')).toBe('\\$2 + A$');
  });

  it('reads `_` after a number as a subscript only before a letter, digit or brace', () => {
    expect(prepareMath('fell to _$4,416_ today')).toBe('fell to _\\$4,416_ today');
    expect(prepareMath('__$5__ and $5_ ok')).toBe('__\\$5__ and \\$5_ ok');
    expect(prepareMath('$5_x$ and $2_{10}$')).toBe('$5_x$ and $2_{10}$');
  });

  it('keeps display math and escaped dollars as they are', () => {
    expect(prepareMath('$$\nE = mc^2\n$$')).toBe('$$\nE = mc^2\n$$');
    expect(prepareMath('costs \\$5')).toBe('costs \\$5');
  });

  it('converts LaTeX delimiters', () => {
    expect(prepareMath('where \\(r = 0.05\\) is the rate')).toBe('where $r = 0.05$ is the rate');
    expect(prepareMath('\\[\\sigma = \\sqrt{v}\\]')).toBe('$$\n\\sigma = \\sqrt{v}\n$$');
    expect(prepareMath('Variance:\n\\[\n  \\sigma^2 = E[X^2]\n\\]\nwhere')).toBe(
      'Variance:\n$$\n\\sigma^2 = E[X^2]\n$$\nwhere',
    );
  });

  it('never escapes converted math as a price', () => {
    expect(prepareMath('solve \\(5x + 3 = 18\\) for x, then pay $5')).toBe('solve $5x + 3 = 18$ for x, then pay \\$5');
    expect(prepareMath('\\(4,416\\) and $4,270')).toBe('$4,416$ and \\$4,270');
  });

  it('keeps display math inside its list item or quote', () => {
    expect(prepareMath('1. The rate is\n   \\[\n   r = 0.05\n   \\]\n2. Next')).toBe(
      '1. The rate is\n   $$\n   r = 0.05\n   $$\n2. Next',
    );
    expect(prepareMath('> Variance:\n> \\[\\sigma^2\\]\n> done')).toBe('> Variance:\n> $$\n> \\sigma^2\n> $$\n> done');
    expect(prepareMath('> \\[\n> a \\\\\n> b\n> \\]\n> after')).toBe('> $$\n> a \\\\\n> b\n> $$\n> after');
  });

  it('turns display math that shares its line into one-line inline math', () => {
    expect(prepareMath('| a | b |\n|---|---|\n| \\[x^2\\] | 1 |')).toBe('| a | b |\n|---|---|\n| $$x^2$$ | 1 |');
    expect(prepareMath('so \\[\n  a = b\n\\] holds')).toBe('so $$a = b$$ holds');
    expect(prepareMath('\\[x\\] is the rate')).toBe('$$x$$ is the rate');
    expect(prepareMath('## Rate \\[r\\]')).toBe('## Rate $$r$$');
  });

  it('leaves an empty pair of delimiters alone', () => {
    expect(prepareMath('- \\[ \\] buy at $5\n- \\( \\)')).toBe('- \\[ \\] buy at \\$5\n- \\( \\)');
  });

  it('leaves code alone, including a block that is still streaming', () => {
    expect(prepareMath('Run `echo $5` then pay $5')).toBe('Run `echo $5` then pay \\$5');
    expect(prepareMath('```python\nprice = "$5"\n```\n$5')).toBe('```python\nprice = "$5"\n```\n\\$5');
    expect(prepareMath('```python\nprice = "$5"')).toBe('```python\nprice = "$5"');
    expect(prepareMath('~~~\n$5 and')).toBe('~~~\n$5 and');
    expect(prepareMath('`\\(x\\)` and \\(y\\)')).toBe('`\\(x\\)` and $y$');
  });

  it('closes code spans only on a backtick run of the same length', () => {
    expect(prepareMath('Run ``echo `$5` `` then pay $5')).toBe('Run ``echo `$5` `` then pay \\$5');
    expect(prepareMath('Use ``$5 ` $6`` then pay $5')).toBe('Use ``$5 ` $6`` then pay \\$5');
  });

  it('closes a fence only on a run at least as long as the one that opened it', () => {
    expect(prepareMath('````md\n```\nprice = $5\n```\n````\n$5')).toBe('````md\n```\nprice = $5\n```\n````\n\\$5');
    expect(prepareMath('````\n```\n$5')).toBe('````\n```\n$5');
  });

  it('leaves fences inside quotes and list items alone', () => {
    expect(prepareMath('> ```\n> $5\n> ```\n> $5')).toBe('> ```\n> $5\n> ```\n> \\$5');
    expect(prepareMath('1. ```bash\n   echo $5\n   ```\n2. Pay $5')).toBe(
      '1. ```bash\n   echo $5\n   ```\n2. Pay \\$5',
    );
  });

  it('leaves indented code blocks alone', () => {
    expect(prepareMath('Example:\n\n    total = $5 + $10\n\nPay $5')).toBe(
      'Example:\n\n    total = $5 + $10\n\nPay \\$5',
    );
    expect(prepareMath('text\n\n\tcode $5\n\nPay $5')).toBe('text\n\n\tcode $5\n\nPay \\$5');
    expect(prepareMath('- a\n\n      code $5 and $6\n\n  para $5')).toBe('- a\n\n      code $5 and $6\n\n  para \\$5');
  });

  it('reads indented text in a list, or right after a paragraph, as prose', () => {
    expect(prepareMath('- Gold\n  - Support\n\n    Held at $4,400 and $4,350')).toBe(
      '- Gold\n  - Support\n\n    Held at \\$4,400 and \\$4,350',
    );
    expect(prepareMath('10. item\n\n    Price $5 and $6')).toBe('10. item\n\n    Price \\$5 and \\$6');
    expect(prepareMath('Pay\n    $5 and $6')).toBe('Pay\n    \\$5 and \\$6');
  });

  it('escapes prices in links, headings and tables', () => {
    expect(prepareMath('See [the $5 plan](https://example.com/?p=$5) for $10')).toBe(
      'See [the \\$5 plan](https://example.com/?p=\\$5) for \\$10',
    );
    expect(prepareMath('## Gold: $4,416 to $4,270')).toBe('## Gold: \\$4,416 to \\$4,270');
    expect(prepareMath('| Level | Price |\n|---|---|\n| R1 | $4,500 |\n|$5|$6|')).toBe(
      '| Level | Price |\n|---|---|\n| R1 | \\$4,500 |\n|\\$5|\\$6|',
    );
  });

  it('handles text that is still streaming', () => {
    expect(prepareMath('Price: $4,41')).toBe('Price: \\$4,41');
    expect(prepareMath('solve \\(5x + ')).toBe('solve \\(5x + ');
    expect(prepareMath('\\[\nx = ')).toBe('\\[\nx = ');
    expect(prepareMath('$2_{10}')).toBe('$2_{10}');
    expect(prepareMath('Code `$5`, math \\(2x\\), price $5.')).toBe('Code `$5`, math $2x$, price \\$5.');
  });

  it('returns text without dollars untouched', () => {
    const text = 'Nothing to see here.';
    expect(prepareMath(text)).toBe(text);
  });
});
