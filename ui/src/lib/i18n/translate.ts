export type Dictionary = { [key: string]: string | Dictionary };
export type Params = Record<string, string | number>;

/** Looks up a dot-separated key, e.g. `settings.logging.title`. */
export function lookup(dict: Dictionary, key: string): string | undefined {
  let node: string | Dictionary | undefined = dict;
  for (const part of key.split('.')) {
    if (node === undefined || typeof node === 'string') return undefined;
    node = node[part];
  }
  return typeof node === 'string' ? node : undefined;
}

/** Replaces `{name}` placeholders with values from `params`. */
export function interpolate(template: string, params?: Params): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(params[name]) : match,
  );
}

/** Lists every leaf key of a dictionary, sorted. */
export function flattenKeys(dict: Dictionary, prefix = ''): string[] {
  const keys: string[] = [];
  for (const [k, v] of Object.entries(dict)) {
    const path = prefix ? `${prefix}.${k}` : k;
    if (typeof v === 'string') keys.push(path);
    else keys.push(...flattenKeys(v, path));
  }
  return keys.sort();
}
