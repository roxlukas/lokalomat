<script lang="ts">
  import Server from '@lucide/svelte/icons/server';
  import Copy from '@lucide/svelte/icons/copy';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import Plus from '@lucide/svelte/icons/plus';
  import { getLocale, t } from '../lib/i18n/index.svelte';
  import { inTauri, ipc, type ServerLogEntry, type ServerStatus } from '../lib/ipc';
  import { describeError } from '../lib/feedback';
  import { settings } from '../lib/settings.svelte';
  import ViewHost from '../lib/components/ViewHost.svelte';
  import { registeredServerTabs } from '../lib/edition/registry';
  import { readLocal, writeLocal } from '../lib/storage';

  /** Serwer (Expert, PRD §15 point 9, ADR 0025): the local OpenAI-compatible API for agents. */
  const SNIPPETS = ['curl', 'python', 'env'] as const;
  type Snippet = (typeof SNIPPETS)[number];

  let server = $state<ServerStatus | null>(null);
  let log = $state<ServerLogEntry[]>([]);
  let error = $state('');
  let busy = $state(false);
  let keyName = $state('');
  let fresh = $state<{ id: string; secret: string } | null>(null);
  let copied = $state<string | null>(null);
  let confirmRevoke = $state<string | null>(null);
  let snippet = $state<Snippet>('curl');
  let port = $state(47822);
  // The free edition has the server only; Pro adds tabs (agents, MCP; ADR 0035).
  const extraTabs = registeredServerTabs();
  const TABS = ['server', ...extraTabs.map((x) => x.id)];
  const saved = readLocal<string>('lokalsi.server-tab', 'server');
  let tab = $state<string>(TABS.includes(saved) ? saved : 'server');
  $effect(() => writeLocal('lokalsi.server-tab', tab));

  let url = $derived(server?.url ?? `http://127.0.0.1:${server?.port ?? port}/v1`);
  let snippetText = $derived(snippetFor(snippet, url));

  async function refresh() {
    try {
      server = await ipc.getServer();
      port = server.port;
      log = server.running ? await ipc.serverLog() : [];
    } catch (e) {
      if (inTauri) error = describeError(e);
    }
  }

  $effect(() => {
    void refresh();
    // The request list follows what agents do while the view is open.
    const timer = setInterval(() => {
      if (server?.running) void ipc.serverLog().then((l) => (log = l)).catch(() => undefined);
    }, 3000);
    return () => clearInterval(timer);
  });

  async function act(fn: () => Promise<ServerStatus | void>) {
    error = '';
    busy = true;
    try {
      const s = await fn();
      if (s) server = s;
    } catch (e) {
      error = describeError(e);
    } finally {
      busy = false;
    }
  }

  /**
   * Changes one server setting from the backend's current state, then reloads
   * the settings store: `set_server` saves settings, and a stale copy in the UI
   * would switch the server off again with the next settings change.
   */
  function change(patch: { enabled?: boolean; lan?: boolean; port?: number }) {
    void act(async () => {
      const now = await ipc.getServer();
      const next = await ipc.setServer(patch.enabled ?? now.enabled, patch.lan ?? now.lan, patch.port);
      await settings.load();
      return next;
    });
  }

  function setEnabled(enabled: boolean) {
    change({ enabled });
  }

  function setLan(lan: boolean) {
    change({ lan });
  }

  function savePort() {
    if (port >= 1024 && port <= 65535 && port !== server?.port) change({ port });
  }

  async function createKey() {
    const name = keyName.trim();
    if (!name) return;
    await act(async () => {
      const made = await ipc.createApiKey(name);
      fresh = { id: made.key.id, secret: made.secret };
      keyName = '';
      return ipc.getServer();
    });
  }

  async function copy(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = what;
      setTimeout(() => copied === what && (copied = null), 1500);
    } catch {
      error = t('server.copyFailed');
    }
  }

  async function copyKey(id: string) {
    try {
      await copy(await ipc.apiKeySecret(id), `key:${id}`);
    } catch (e) {
      error = describeError(e);
    }
  }

  function snippetFor(kind: Snippet, base: string): string {
    const key = 'TWOJ_KLUCZ';
    if (kind === 'python')
      return `from openai import OpenAI\n\nclient = OpenAI(base_url="${base}", api_key="${key}")\nanswer = client.chat.completions.create(\n    model="default",\n    messages=[{"role": "user", "content": "Cześć!"}],\n)\nprint(answer.choices[0].message.content)`;
    if (kind === 'env') return `OPENAI_BASE_URL=${base}\nOPENAI_API_KEY=${key}`;
    return `curl ${base}/chat/completions \\\n  -H "Authorization: Bearer ${key}" \\\n  -H "Content-Type: application/json" \\\n  -d '{"model": "default", "messages": [{"role": "user", "content": "Cześć!"}]}'`;
  }

  function when(secs: number | null): string {
    if (!secs) return t('server.never');
    return new Intl.DateTimeFormat(getLocale(), { dateStyle: 'short', timeStyle: 'short' }).format(new Date(secs * 1000));
  }

  function clock(secs: number): string {
    return new Intl.DateTimeFormat(getLocale(), { timeStyle: 'medium' }).format(new Date(secs * 1000));
  }
</script>

<div class="view">
  <header class="head">
    <Server size={22} aria-hidden="true" />
    <div>
      <h1>{t('views.server.title')}</h1>
      <p class="muted">{t('views.server.body')}</p>
    </div>
  </header>

  {#if TABS.length > 1}
  <div class="tabs" role="tablist" aria-label={t('views.server.title')}>
    {#each TABS as id (id)}
      <button type="button" role="tab" id={`tab-${id}`} aria-selected={tab === id} class="tab" class:active={tab === id} onclick={() => (tab = id)}>{t(id === 'server' ? 'server.tabs.server' : (extraTabs.find((x) => x.id === id)?.labelKey ?? id))}</button>
    {/each}
  </div>
  {/if}

  {#if tab !== 'server'}
    <ViewHost load={extraTabs.find((x) => x.id === tab)?.load} />
  {:else}
  <section aria-labelledby="srv-main">
    <h2 id="srv-main" class="sr-only">{t('views.server.title')}</h2>
    <label class="switch">
      <input type="checkbox" checked={server?.enabled ?? false} disabled={busy || !inTauri} onchange={(e) => setEnabled(e.currentTarget.checked)} />
      <span>{t('server.enable')}</span>
    </label>
    {#if server?.error}
      <p class="note note-error" role="alert">{t(`server.errors.${server.error}`)}</p>
    {/if}
    {#if server?.running}
      <div class="address">
        <code>{url}</code>
        <button type="button" class="btn small" onclick={() => copy(url, 'url')}>
          <Copy size={14} aria-hidden="true" />{copied === 'url' ? t('jobs.copied') : t('server.copy')}
        </button>
      </div>
      <p class="muted small">{t('server.autoload')}</p>
    {/if}
    <label class="switch" title={t('server.lanTip')}>
      <input type="checkbox" checked={server?.lan ?? false} disabled={busy || !inTauri} onchange={(e) => setLan(e.currentTarget.checked)} />
      <span>{t('server.lan')}</span>
    </label>
    {#if server?.lan}
      <p class="note note-warn small">{t('server.lanWarn')}</p>
      {#if server.lan_url}<p class="muted small">{t('server.lanAddress', { address: server.lan_url })}</p>{/if}
    {/if}
    <label class="inline">
      <span class="muted small">{t('server.port')}</span>
      <input type="number" min="1024" max="65535" bind:value={port} onchange={savePort} disabled={!inTauri} />
    </label>
  </section>

  <section aria-labelledby="srv-keys">
    <h2 id="srv-keys">{t('server.keys')}</h2>
    <p class="muted small">{t('server.keysBody')}</p>
    <form class="inline" onsubmit={(e) => (e.preventDefault(), void createKey())}>
      <label class="sr-only" for="key-name">{t('server.keyName')}</label>
      <input id="key-name" type="text" maxlength="60" bind:value={keyName} placeholder={t('server.keyNamePlaceholder')} />
      <button type="submit" class="btn" disabled={busy || !keyName.trim() || !inTauri}>
        <Plus size={14} aria-hidden="true" />{t('server.newKey')}
      </button>
    </form>
    {#if fresh}
      <div class="fresh" role="status">
        <p><strong>{t('server.freshTitle')}</strong></p>
        <code>{fresh.secret}</code>
        <div class="actions">
          <button type="button" class="btn small btn-primary" onclick={() => copy(fresh!.secret, 'fresh')}>
            <Copy size={14} aria-hidden="true" />{copied === 'fresh' ? t('jobs.copied') : t('server.copy')}
          </button>
          <button type="button" class="btn small" onclick={() => (fresh = null)}>{t('server.hide')}</button>
        </div>
      </div>
    {/if}
    {#if !server || server.keys.length === 0}
      <p class="muted small">{t('server.noKeys')}</p>
    {:else}
      <ul class="keys">
        {#each server.keys as k (k.id)}
          <li>
            <KeyRound size={16} aria-hidden="true" />
            <div class="key-text">
              <strong>{k.name}</strong>
              <span class="muted small">{t('server.keyMeta', { created: when(k.created_at), used: when(k.last_used_at), count: k.requests })}</span>
            </div>
            <button type="button" class="btn small" onclick={() => copyKey(k.id)} title={t('server.copyKeyTip')}>
              <Copy size={14} aria-hidden="true" />{copied === `key:${k.id}` ? t('jobs.copied') : t('server.copy')}
            </button>
            {#if confirmRevoke === k.id}
              <button type="button" class="btn small danger" onclick={() => act(() => ipc.revokeApiKey(k.id)).then(() => (confirmRevoke = null))}>{t('server.confirmRevoke')}</button>
            {:else}
              <button type="button" class="btn small" aria-label={t('server.revoke')} title={t('server.revoke')} onclick={() => (confirmRevoke = k.id)}>
                <Trash2 size={14} aria-hidden="true" />
              </button>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <section aria-labelledby="srv-use">
    <h2 id="srv-use">{t('server.howTo')}</h2>
    <div class="chip-group" role="radiogroup" aria-label={t('server.howTo')}>
      {#each SNIPPETS as s (s)}
        <button type="button" role="radio" aria-checked={snippet === s} class="chip" class:active={snippet === s} onclick={() => (snippet = s)}>{t(`server.snippets.${s}`)}</button>
      {/each}
    </div>
    <pre><code>{snippetText}</code></pre>
    <div class="actions">
      <button type="button" class="btn small" onclick={() => copy(snippetText, 'snippet')}>
        <Copy size={14} aria-hidden="true" />{copied === 'snippet' ? t('jobs.copied') : t('server.copy')}
      </button>
      <span class="muted small">{t('server.snippetHint')}</span>
    </div>
  </section>

  <section aria-labelledby="srv-log">
    <h2 id="srv-log">{t('server.log')}</h2>
    {#if log.length === 0}
      <p class="muted small">{t('server.noLog')}</p>
    {:else}
      <table>
        <thead>
          <tr><th>{t('server.cols.time')}</th><th>{t('server.cols.key')}</th><th>{t('server.cols.path')}</th><th>{t('server.cols.model')}</th><th>{t('server.cols.status')}</th><th>{t('server.cols.duration')}</th></tr>
        </thead>
        <tbody>
          {#each [...log].reverse() as l, i (i)}
            <tr class:bad={l.status >= 400}>
              <td>{clock(l.at)}</td><td>{l.key}</td><td><code>{l.path}</code></td><td>{l.model ?? t('server.defaultModel')}</td>
              <td>{l.status}</td><td>{(l.duration_ms / 1000).toLocaleString(getLocale(), { maximumFractionDigits: 1 })} s</td>
            </tr>
          {/each}
        </tbody>
      </table>
      <p class="muted small">{t('server.logNote')}</p>
    {/if}
  </section>

  {/if}

  {#if error}<p class="note note-error" role="alert">{error}</p>{/if}
</div>

<style>
  .view {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    height: 100%;
    max-width: 900px;
    margin: 0 auto;
    padding: var(--space-5) var(--space-4);
    overflow-y: auto;
  }

  .tabs {
    display: flex;
    gap: var(--space-1);
    border-bottom: 1px solid var(--border);
  }

  .tab {
    padding: var(--space-2) var(--space-3);
    border: 0;
    border-bottom: 2px solid transparent;
    background: transparent;
    color: var(--text-muted);
  }

  .tab.active {
    border-bottom-color: var(--accent);
    color: var(--text);
  }

  .head {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
  }

  h1 {
    font-size: var(--text-xl);
  }

  h2 {
    font-size: var(--text-m);
  }

  section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-l);
    background: var(--surface);
  }

  .muted {
    color: var(--text-muted);
  }

  .small {
    font-size: var(--text-s);
  }

  .switch,
  .inline,
  .actions,
  .address {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }

  .inline input[type='number'] {
    width: 110px;
  }

  .inline input[type='text'] {
    flex: 1;
    min-width: 200px;
  }

  code,
  pre {
    font-family: var(--font-mono);
    font-size: var(--text-s);
  }

  .address code,
  .fresh code {
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-s);
    background: var(--surface-2);
    overflow-wrap: anywhere;
  }

  .fresh {
    display: grid;
    gap: var(--space-2);
    padding: var(--space-3);
    border-radius: var(--radius-m);
    background: var(--accent-soft);
  }

  pre {
    margin: 0;
    padding: var(--space-3);
    border-radius: var(--radius-m);
    background: var(--surface-2);
    overflow-x: auto;
    white-space: pre;
  }

  .keys {
    display: grid;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .keys li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
  }

  .key-text {
    display: grid;
    flex: 1;
    min-width: 200px;
  }

  .chip-group {
    display: inline-flex;
    align-self: flex-start;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
  }

  .chip {
    padding: 2px var(--space-3);
    border: 1px solid transparent;
    border-radius: var(--radius-pill);
    background: transparent;
    color: var(--text-muted);
    font-size: var(--text-s);
  }

  .chip.active {
    background: var(--surface-2);
    border-color: var(--border-strong);
    color: var(--text);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-s);
    font-variant-numeric: tabular-nums;
  }

  th,
  td {
    padding: var(--space-1) var(--space-2);
    border-bottom: 1px solid var(--border);
    text-align: left;
  }

  th {
    color: var(--text-muted);
    font-weight: 600;
  }

  tr.bad td {
    color: var(--error);
  }

  .btn.small {
    min-height: 28px;
    padding: 0 var(--space-2);
    font-size: var(--text-s);
  }
</style>
