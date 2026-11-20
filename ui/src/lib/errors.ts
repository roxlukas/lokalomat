import { ipc, type LogLevel } from './ipc';

let forwarding = false;

/** Sends a message to the application log; never throws. */
export function logToApp(level: LogLevel, message: string, context?: string): void {
  if (forwarding) return; // avoid loops if logging itself fails
  forwarding = true;
  ipc
    .logFromUi(level, message, context)
    .catch(() => undefined)
    .finally(() => {
      forwarding = false;
    });
}

/** Forwards uncaught frontend errors to the Rust log (PRD §10: logs in the data dir). */
export function installErrorForwarding(): void {
  window.addEventListener('error', (event) => {
    const where = event.filename ? `${event.filename}:${event.lineno}:${event.colno}` : undefined;
    const stack = event.error instanceof Error && event.error.stack ? `\n${event.error.stack}` : '';
    logToApp('error', `${event.message}${stack}`, where);
  });
  window.addEventListener('unhandledrejection', (event) => {
    const reason = event.reason instanceof Error ? (event.reason.stack ?? event.reason.message) : String(event.reason);
    logToApp('error', `unhandled promise rejection: ${reason}`);
  });
}
