import { findView } from './edition/registry';

/** The id of a registered view (`models`, `settings`…). */
export type ViewId = string;

export const DEFAULT_VIEW: ViewId = 'workshop';

export function isViewId(value: string): boolean {
  return findView(value) !== undefined;
}

/** `#/models` → `models`; anything unknown → default view. */
export function parseHash(hash: string): ViewId {
  const id = hash.replace(/^#\/?/, '').split(/[/?]/)[0] ?? '';
  return isViewId(id) ? id : DEFAULT_VIEW;
}

export function toHash(id: ViewId): string {
  return `#/${id}`;
}
