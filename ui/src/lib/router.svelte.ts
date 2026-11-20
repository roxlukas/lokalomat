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

  go(id: ViewId): void {
    if (location.hash !== toHash(id)) location.hash = toHash(id);
    this.current = id;
  }
}

export const router = new Router();
