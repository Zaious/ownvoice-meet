// New versions, asked for only when the user presses "Check for updates" in Settings. An installed
// copy (the Windows installer) updates itself through Tauri's updater, and only when the user
// presses Update; a portable copy or a dev build gets a link to the release page.

import { invoke } from '@tauri-apps/api/core';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { newerRelease } from './version.ts';

export type InstallKind = 'installed' | 'portable' | 'other';

export interface Newer {
  tag: string;
  /** The release page. */
  url: string;
  /** Set when this copy can install it itself. */
  update?: Update;
}

const RELEASE_TAG_URL = 'https://github.com/Zaious/ownvoice-meet/releases/tag/';
export const RELEASES_URL = 'https://github.com/Zaious/ownvoice-meet/releases';

export const installKind = (): Promise<InstallKind> => invoke<InstallKind>('install_kind').catch((): InstallKind => 'other');

/** A newer release, if there is one. Never throws. */
export async function findNewer(current: string, kind: InstallKind): Promise<Newer | null> {
  if (kind === 'installed') {
    try {
      const u = await check({ timeout: 15_000 });
      return u ? { tag: `v${u.version}`, url: RELEASE_TAG_URL + `v${u.version}`, update: u } : null;
    } catch {
      // A build without the update key, no update file for this release, or no network: the plain
      // check below still finds a link.
    }
  }
  return newerRelease(current);
}

/**
 * Download, check the signature, install, and start the new version. `progress` gets 0 to 100 (or
 * null while the size is unknown). On Windows the installer closes the app itself.
 */
export async function installNewer(n: Newer, progress: (pct: number | null) => void): Promise<void> {
  if (!n.update) throw new Error('nothing to install');
  let total = 0, got = 0;
  await n.update.downloadAndInstall((e) => {
    if (e.event === 'Started') total = e.data.contentLength ?? 0;
    else if (e.event === 'Progress') { got += e.data.chunkLength; progress(total ? Math.min(100, Math.round((got / total) * 100)) : null); }
    else progress(100);
  });
  await relaunch();
}
