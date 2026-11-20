// Updates (ADR 0029): the backend checks GitHub at startup when the setting
// allows it and sends `update://available`; the settings can check by hand.
import { openUrl } from '@tauri-apps/plugin-opener';
import { EVENTS, errorMessage, inTauri, ipc, onEvent, type UpdateInfo } from './ipc';
import { describeError } from './feedback';
import { logToApp } from './errors';

export const RELEASES_URL = 'https://github.com/roxlukas/lokalomat/releases/latest';

class Updates {
  available = $state<UpdateInfo | null>(null);
  /** The banner was closed for this version ("Później"). */
  dismissed = $state(false);
  checking = $state(false);
  installing = $state(false);
  /** `done / total` while downloading, when the size is known. */
  progress = $state<number | null>(null);
  /** Result of the last check by hand: null, 'none' or an error text. */
  lastCheck = $state<'none' | 'found' | null>(null);
  error = $state('');

  async init() {
    await onEvent<UpdateInfo>(EVENTS.updateAvailable, (info) => {
      this.available = info;
      this.dismissed = false;
    });
    await onEvent<[number, number]>(EVENTS.updateProgress, ([done, total]) => {
      this.progress = total ? done / total : null;
    });
  }

  async check() {
    if (!inTauri) return;
    this.checking = true;
    this.error = '';
    try {
      this.available = await ipc.checkUpdate();
      this.lastCheck = this.available ? 'found' : 'none';
      this.dismissed = false;
    } catch (e) {
      this.error = describeError(e);
      logToApp('warn', `update check failed: ${errorMessage(e)}`);
    } finally {
      this.checking = false;
    }
  }

  /** Installs and restarts; a `.deb` install opens the release page instead. */
  async install() {
    if (!this.available) return;
    if (this.available.manual) {
      await openUrl(RELEASES_URL).catch(() => undefined);
      return;
    }
    this.installing = true;
    this.error = '';
    this.progress = null;
    try {
      await ipc.installUpdate();
    } catch (e) {
      this.error = describeError(e);
      this.installing = false;
      logToApp('error', `update install failed: ${errorMessage(e)}`);
    }
  }
}

export const updates = new Updates();
