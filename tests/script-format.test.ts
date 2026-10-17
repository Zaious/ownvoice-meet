import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { missingGloss, parse } from '../src/script/format.ts';

describe('script format', () => {
  it('splits sections into tabs and keeps item order', () => {
    const { sections, items } = parse('# One\nHello.\n> 哈囉。\n# Two\n! cue');
    expect(sections).toEqual(['One', 'Two']);
    expect(items.map((i) => i.kind)).toEqual(['section', 'line', 'gloss', 'section', 'cue']);
  });

  it('reads copy blocks verbatim, including backslashes and blank-looking lines', () => {
    const { items } = parse('```copy:cmd\npython C:\\tools\\run.py --x\n```');
    expect(items).toEqual([{ kind: 'copy', name: 'cmd', text: 'python C:\\tools\\run.py --x' }]);
  });

  it('still shows an unclosed copy block', () => {
    expect(parse('```copy:a\nkeep me').items).toEqual([{ kind: 'copy', name: 'a', text: 'keep me' }]);
  });

  it('ignores a byte-order mark and CRLF line ends (Windows editors)', () => {
    expect(parse('\uFEFF# Title\r\nLine\r\n> 行').sections).toEqual(['Title']);
  });

  it('reports lines and questions without a meaning line', () => {
    const { items } = parse('Q: Why?\nBecause.\n> 因為。\nAlone.');
    expect(missingGloss(items)).toEqual(['Why?', 'Alone.']);
  });

  it('the bundled example has a meaning under every line', () => {
    const { sections, items } = parse(readFileSync('examples/scripts/demo.md', 'utf8'));
    expect(sections).toEqual(['Pitch', 'Demo', 'Q&A']);
    expect(missingGloss(items)).toEqual([]);
  });
});

describe('editor template', () => {
  it('parses into its tabs and leaves the Chinese lines for 補對照', async () => {
    const { TEMPLATE } = await import('../src/script/template.ts');
    const { sections, items } = parse(TEMPLATE);
    expect(sections).toEqual(['開場', 'Q&A']);
    expect(missingGloss(items)).toEqual(['寫中文也可以，按「補對照」會幫你產生要唸的英文。', '它不會替你作答，你說的是自己的話。']);
  });
});

describe('JCDL 2026 example', () => {
  it('has the pitch and Q&A tabs and a meaning under every line', () => {
    const { sections, items } = parse(readFileSync('examples/scripts/journal-atlas-jcdl2026.md', 'utf8'));
    expect(sections).toEqual(['Pitch', 'Q&A']);
    expect(missingGloss(items)).toEqual([]);
  });
});
