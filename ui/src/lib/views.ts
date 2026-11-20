import { registeredViews, type ViewDef } from './edition/registry';

export type { ViewDef };

/** Views for the current mode, in rail order (what the edition registered). */
export function visibleViews(expert: boolean): ViewDef[] {
  return registeredViews().filter((v) => expert || !v.expert);
}
