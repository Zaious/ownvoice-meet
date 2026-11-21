import { existsSync, readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

// The product site (site/, served at ownvoice.chroniclecore.com). The questions are written twice, once
// for readers and once as FAQPage data for search engines; they must say the same thing.
const PAGES = [
  { file: 'site/index.html', url: 'https://ownvoice.chroniclecore.com/' },
  { file: 'site/en/index.html', url: 'https://ownvoice.chroniclecore.com/en/' },
];

const decode = (s: string) => s.replace(/&amp;/g, '&').replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"');

describe.each(PAGES)('$file', ({ file, url }) => {
  const html = readFileSync(file, 'utf8');

  it('has its canonical address, both languages and a share card', () => {
    expect(html).toContain(`<link rel="canonical" href="${url}">`);
    expect(html).toContain('hreflang="zh-Hant" href="https://ownvoice.chroniclecore.com/"');
    expect(html).toContain('hreflang="en" href="https://ownvoice.chroniclecore.com/en/"');
    expect(html).toContain(`<meta property="og:url" content="${url}">`);
    expect(html).toContain('<meta property="og:image" content="https://ownvoice.chroniclecore.com/media/og.png">');
    expect(html).toContain('<meta name="twitter:card" content="summary_large_image">');
    const desc = /<meta name="description" content="([^"]+)">/.exec(html)?.[1] ?? '';
    expect(desc.length).toBeGreaterThan(50);
  });

  it('says the same in its questions and its FAQ data', () => {
    const blocks = [...html.matchAll(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/g)].map((m) => JSON.parse(m[1] ?? '{}'));
    const faq = blocks.find((b) => b['@type'] === 'FAQPage');
    expect(blocks.find((b) => b['@type'] === 'SoftwareApplication')?.url).toBe(url);
    const data = (faq?.mainEntity ?? []).map((q: { name: string; acceptedAnswer: { text: string } }) => [q.name, q.acceptedAnswer.text]);
    const shown = [...html.matchAll(/<details><summary>([\s\S]*?)<\/summary><p>([\s\S]*?)<\/p><\/details>/g)].map((m) => [
      decode(m[1] ?? ''),
      decode((m[2] ?? '').replace(/<[^>]+>/g, '')),
    ]);
    expect(shown.length).toBeGreaterThan(5);
    expect(shown).toEqual(data);
  });

  it('only uses pictures that the deploy provides', () => {
    for (const m of html.matchAll(/(?:src|href)="(?:\.\.\/)?(media\/[^"]+)"/g)) {
      const path = m[1] ?? '';
      const inSite = existsSync(`site/${path}`);
      const copiedAtDeploy = ['media/demo.gif', 'media/overview.png'].includes(path);
      expect(inSite || copiedAtDeploy, path).toBe(true);
    }
  });
});
