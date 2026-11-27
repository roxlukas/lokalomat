/**
 * Context window choices offered in Expert settings and on the Server view
 * (ADR 0042). `null` = automatic, `CONTEXT_MAX` = the model's own maximum.
 */
export const CONTEXT_MAX = 0;
export const CONTEXT_SIZES = [8192, 16384, 32768, 65536, 131072] as const;

/** `<select>` value for a stored choice. */
export function contextOption(size: number | null): string {
  return size === null ? 'auto' : String(size);
}

/** Stored choice for a `<select>` value. */
export function contextChoice(option: string): number | null {
  return option === 'auto' ? null : Number(option);
}

/** Choices to list: the fixed ones plus an older saved value (e.g. 4k) so the select shows it. */
export function contextChoices(current: number | null): number[] {
  const list: number[] = [...CONTEXT_SIZES];
  if (current !== null && current !== CONTEXT_MAX && !list.includes(current)) {
    list.push(current);
    list.sort((a, b) => a - b);
  }
  return list;
}
