// The bilingual script format, one item per line (ported from the PyQt prototype's scriptview.py):
//   # Title        a tab and section header
//   ## Sub         a sub-header
//   ! cue          a stage direction
//   Q: question    a question
//   > 中文          the meaning of the line above
//   ```copy:name   a block shown boxed with a copy button, until ```
//   anything else  a line to read aloud

export type Item =
  | { kind: 'section'; text: string; index: number }
  | { kind: 'sub' | 'cue' | 'gloss' | 'question' | 'line'; text: string }
  | { kind: 'copy'; name: string; text: string };

export interface Parsed {
  sections: string[];
  items: Item[];
}

export function parse(source: string): Parsed {
  const sections: string[] = [];
  const items: Item[] = [];
  let block: { name: string; lines: string[] } | null = null;
  for (const raw of source.replace(/^﻿/, '').split(/\r?\n/)) {
    const line = raw.trimEnd();
    if (block) {
      if (line.trim() === '```') {
        items.push({ kind: 'copy', name: block.name, text: block.lines.join('\n') });
        block = null;
      } else {
        block.lines.push(line);
      }
      continue;
    }
    if (!line.trim()) continue;
    if (line.startsWith('```copy:')) block = { name: line.slice(8).trim(), lines: [] };
    else if (line.startsWith('# ')) {
      items.push({ kind: 'section', text: line.slice(2).trim(), index: sections.length });
      sections.push(line.slice(2).trim());
    } else if (line.startsWith('## ')) items.push({ kind: 'sub', text: line.slice(3).trim() });
    else if (line.startsWith('! ')) items.push({ kind: 'cue', text: line.slice(2) });
    else if (line.startsWith('> ')) items.push({ kind: 'gloss', text: line.slice(2) });
    else if (line.startsWith('Q: ')) items.push({ kind: 'question', text: line.slice(3) });
    else items.push({ kind: 'line', text: line });
  }
  // An unclosed block still shows, so a typo never hides content.
  if (block) items.push({ kind: 'copy', name: block.name, text: block.lines.join('\n') });
  return { sections, items };
}

/** Lines to read that have no meaning line under them (for authors checking a script). */
export function missingGloss(items: Item[]): string[] {
  const out: string[] = [];
  items.forEach((it, i) => {
    if ((it.kind === 'line' || it.kind === 'question') && items[i + 1]?.kind !== 'gloss') out.push(it.text);
  });
  return out;
}
