/**
 * Where the phone's chat runs: on the phone itself or on the paired computer
 * (ADR 0038). The choice is kept on the phone.
 */
import { readLocal, writeLocal } from '../lib/storage';

export type Source = 'local' | 'computer';

const KEY = 'lokalsi.mobile.source';

class SourceState {
  /** The model on the phone can run here (the Android app, not a browser). */
  localAvailable = $state(false);
  /** Settings, "Manage models": the phone chat opens on the list of models. */
  openModels = $state(false);
  #value = $state<Source | null>(readLocal<Source | null>(KEY, null));

  /** What the user chose, or the sensible default: the computer when paired, else the phone. */
  current(paired: boolean): Source {
    if (this.#value === 'local' && this.localAvailable) return 'local';
    if (this.#value === 'computer' && paired) return 'computer';
    return paired ? 'computer' : this.localAvailable ? 'local' : 'computer';
  }

  set(value: Source) {
    this.#value = value;
    writeLocal(KEY, value);
  }
}

export const source = new SourceState();
