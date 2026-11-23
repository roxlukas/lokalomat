import type { Modality } from './ipc';
import { parseHash, toHash, type ViewId } from './routes';

class Router {
  current = $state<ViewId>(parseHash(typeof location === 'undefined' ? '' : location.hash));

  constructor() {
    if (typeof window !== 'undefined') {
      window.addEventListener('hashchange', () => {
        this.current = parseHash(location.hash);
      });
    }
  }

  /** Kind of models the Models view should show first (set by "go to Models" buttons). */
  modelsModality: Modality | null = null;

  /** Open the Models view filtered to one kind of model. */
  goModels(modality: Modality): void {
    this.modelsModality = modality;
    this.go('models');
  }

  /** The pending filter, once. */
  takeModelsModality(): Modality | null {
    const m = this.modelsModality;
    this.modelsModality = null;
    return m;
  }

  go(id: ViewId): void {
    if (location.hash !== toHash(id)) location.hash = toHash(id);
    this.current = id;
  }
}

export const router = new Router();
