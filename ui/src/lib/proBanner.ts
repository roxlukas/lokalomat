// The Pro banner can be closed; it comes back after a while (ADR 0035).
const KEY = 'lokalsi.pro-banner-closed';
export const REMIND_AFTER_MS = 30 * 24 * 60 * 60 * 1000;

export function isBannerClosed(now = Date.now()): boolean {
  try {
    const at = Number(localStorage.getItem(KEY));
    return Number.isFinite(at) && at > 0 && now - at < REMIND_AFTER_MS;
  } catch {
    return false;
  }
}

export function closeBanner(now = Date.now()): void {
  try {
    localStorage.setItem(KEY, String(now));
  } catch {
    /* storage can be blocked: the banner then simply stays closed for this session only */
  }
}
