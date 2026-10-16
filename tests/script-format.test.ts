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
