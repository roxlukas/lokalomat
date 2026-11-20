import { beforeEach, describe, expect, it } from 'vitest';
import { resetRegistry, registerView } from './edition/registry';
import { DEFAULT_VIEW, parseHash, toHash } from './routes';

const IDS = ['workshop', 'gallery', 'models', 'settings'];

describe('routes', () => {
  beforeEach(() => {
    resetRegistry();
    IDS.forEach((id, order) =>
      registerView({ id, labelKey: `nav.${id}`, icon: (() => {}) as never, order, load: async () => ({ default: (() => {}) as never }) }),
    );
  });

  it('parses registered views', () => {
    expect(parseHash('#/models')).toBe('models');
    expect(parseHash('#settings')).toBe('settings');
    expect(parseHash('#/gallery/123?x=1')).toBe('gallery');
  });

  it('falls back to the default view', () => {
    expect(parseHash('')).toBe(DEFAULT_VIEW);
    expect(parseHash('#/nope')).toBe(DEFAULT_VIEW);
  });

  it('does not know a view the edition did not register', () => {
    resetRegistry();
    expect(parseHash('#/gallery')).toBe(DEFAULT_VIEW);
  });

  it('round-trips every view', () => {
    for (const id of IDS) expect(parseHash(toHash(id))).toBe(id);
  });
});
