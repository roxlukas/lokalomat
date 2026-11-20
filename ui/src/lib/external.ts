import { openUrl } from '@tauri-apps/plugin-opener';
import { inTauri } from './ipc';
import { logToApp } from './errors';

/** Where "Lokalomat Pro" links lead; becomes the purchase page later. */
export const PRO_URL = 'https://aitransformation.pl';

/**
 * Click handler for links: in the desktop app they open in the system browser,
 * not in the WebView. In a plain browser the link works as usual.
 */
export async function openExternal(e: MouseEvent, url: string): Promise<void> {
  if (!inTauri) return;
  e.preventDefault();
  await openUrl(url).catch((err) => logToApp('warn', `cannot open ${url}: ${String(err)}`));
}
