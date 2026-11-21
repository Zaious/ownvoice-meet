import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

// .seg is a caption row. A toolbar style that once reused the name cut every caption line to
// 16em (0.2.0); rules that shorten or clip text must not land on the rows.
describe('caption rows', () => {
  it('are never clipped by a rule meant for something else', () => {
    const css = readFileSync('src/styles.css', 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
    const rules = [...css.matchAll(/([^{}]+)\{([^}]*)\}/g)].map((m) => ({ sel: m[1] ?? '', body: m[2] ?? '' }));
    const isRow = (s: string) => /(^|\s)\.seg(?![\w-])/.test(s) && !/\.seg\S*\s+\./.test(s);
    const onRows = rules.filter(({ sel }) => sel.split(',').some((s) => isRow(s.trim())));
    expect(onRows.length).toBeGreaterThan(0);
    for (const { sel, body } of onRows) {
      expect(body, `rule "${sel.trim()}"`).not.toMatch(/white-space:\s*nowrap|max-width|text-overflow/);
    }
  });
});
