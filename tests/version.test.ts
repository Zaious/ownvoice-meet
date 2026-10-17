import { describe, expect, it } from 'vitest';
import { isNewer, newerRelease } from '../src/settings/version.ts';

describe('release versions', () => {
  it('compares numbers, not text', () => {
    expect(isNewer('v0.10.0', '0.9.9')).toBe(true);
    expect(isNewer('v0.1.0', '0.1.0')).toBe(false);
    expect(isNewer('0.1.0', '0.2.0')).toBe(false);
  });

  it('a release beats its pre-release, not the other way round', () => {
    expect(isNewer('v0.2.0', '0.2.0-beta.1')).toBe(true);
    expect(isNewer('v0.2.0-beta.1', '0.2.0')).toBe(false);
  });

  it('ignores tags that are not versions', () => {
    expect(isNewer('nightly', '0.1.0')).toBe(false);
  });
});

describe('the release-page check', () => {
  const reply = (body: unknown, ok = true) => (async () => ({ ok, json: async () => body })) as unknown as typeof fetch;

  it('finds a newer published release', async () => {
    const r = await newerRelease('0.1.0', reply({ tag_name: 'v0.2.0', html_url: 'https://example.test/v0.2.0' }));
    expect(r).toEqual({ tag: 'v0.2.0', url: 'https://example.test/v0.2.0' });
  });

  it('skips drafts, pre-releases, the same version and errors', async () => {
    expect(await newerRelease('0.1.0', reply({ tag_name: 'v0.2.0', html_url: 'u', draft: true }))).toBeNull();
    expect(await newerRelease('0.1.0', reply({ tag_name: 'v0.2.0', html_url: 'u', prerelease: true }))).toBeNull();
    expect(await newerRelease('0.1.0', reply({ tag_name: 'v0.1.0', html_url: 'u' }))).toBeNull();
    // A private repository answers 404 to a visitor.
    expect(await newerRelease('0.1.0', reply({}, false))).toBeNull();
    expect(await newerRelease('0.1.0', (async () => { throw new Error('offline'); }) as unknown as typeof fetch)).toBeNull();
  });
});
