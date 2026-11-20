/** Human-readable byte size, e.g. `1,4 MB` (locale-aware decimal separator). */
export function formatBytes(bytes: number, locale = 'pl'): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1;
  return `${value.toLocaleString(locale, { maximumFractionDigits: digits, minimumFractionDigits: digits })} ${units[unit]}`;
}

/** Megabytes (as used for memory budgets) → `23,5 GB`. */
export function formatMb(mb: number, locale = 'pl'): string {
  return formatBytes(mb * 1024 * 1024, locale);
}

/** `12,3 MB/s` */
export function formatSpeed(bytesPerSec: number, locale = 'pl'): string {
  return `${formatBytes(bytesPerSec, locale)}/s`;
}

/** Remaining time in seconds, `null` when unknown. */
export function etaSeconds(done: number, total: number, bytesPerSec: number): number | null {
  if (bytesPerSec <= 0 || total <= 0 || done >= total) return null;
  return Math.ceil((total - done) / bytesPerSec);
}

/** Compact duration: `45 s`, `12 min`, `1 h 20 min`. */
export function formatDuration(seconds: number): string {
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return m ? `${h} h ${m} min` : `${h} h`;
}

/** `45%` */
export function formatPercent(fraction: number, locale = 'pl'): string {
  return `${Math.round(fraction * 100).toLocaleString(locale)}%`;
}

/** `lokalomat-diagnostics-20261111-1430.zip` */
export function diagnosticsFileName(now = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  const stamp = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}`;
  return `lokalomat-diagnostics-${stamp}.zip`;
}
