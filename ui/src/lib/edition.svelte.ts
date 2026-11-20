// Edition (ADR 0035): the free Lokalomat or Lokalomat Pro. Until the backend
// answers we assume Pro, so the Pro banner never flashes in the paid app.
import { errorMessage, ipc, type Edition } from './ipc';
import { logToApp } from './errors';

class EditionStore {
  value = $state<Edition>('pro');
  loaded = $state(false);

  get isPro(): boolean {
    return this.value === 'pro';
  }

  async load(): Promise<void> {
    try {
      this.value = await ipc.getEdition();
    } catch (e) {
      logToApp('warn', `get_edition failed: ${errorMessage(e)}`);
      this.value = 'free';
    }
    this.loaded = true;
  }
}

export const edition = new EditionStore();
