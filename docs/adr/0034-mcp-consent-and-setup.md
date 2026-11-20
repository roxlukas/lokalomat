# ADR 0034: Permission for MCP programs, client list and one-click setup (M7, part 3)

- Status: accepted (owner, 2026-11-18: ask once in the app when a new client first connects;
  allowed clients are listed and can be revoked; one-click "Add to Claude Code").
- Date: 2026-11-18

## Context

ADR 0033 lets any program with an access key use every mode through MCP. The owner wants the
user to decide which programs may do that, and wants connecting Claude Code to be one click.
MCP over HTTP has no sessions here (ADR 0033), and `clientInfo.name` is whatever the program
says it is.

## Decision

1. **A program is asked about once.** At `initialize`, if no client with that
   `clientInfo.name` was allowed with that key before, the app shows a dialog over
   everything ("Zezwolić programowi „claude-code”?" with the key name) and holds the
   `initialize` answer until the user decides. *Allow* is remembered; *Don't allow*, Escape
   and no answer within 120 seconds refuse it (a JSON-RPC error, code -32001) and are not
   remembered, so the program asks again on its next start. Programs that connect at the same
   time with the same key and name share one question. The window asks for attention when it
   is in the background. The dialog works in Basic and Expert mode; it is the only part of
   this feature that Basic users meet.
2. **Allowed clients are stored** per key and name (`mcp_clients`, migration 9: version,
   time allowed, last seen). Revoking a key removes its clients.
3. **Every later request is checked against the key**: it needs at least one allowed client.
   `ping` needs nothing. After the user withdraws a client, that key's tool calls fail with
   "No permission. Connect again and allow it in Lokalomat." until a program connects and is
   allowed again. Plain REST calls (`/v1/...`) are not affected: for those the key is the
   permission, as in ADR 0025.
4. **Serwer › MCP tab** (Expert, like the rest of the Server view):
   - "Dodaj do Claude Code": pick a key, pick HTTP or the bridge, and either run
     `claude mcp add --scope user …` for the user or copy the command. The command shows the
     key masked; copying gives the real text. The app only runs the command on the click, replaces an
     earlier `lokalomat` entry, never returns the output (it can hold the key) and logs only
     redacted text. If `claude` is not found, only copying is offered.
   - "Inne programy MCP": ready JSON (`mcpServers`) for Claude Desktop, Cursor and similar.
   - "Dozwolone programy": the list with last-seen time and "Cofnij zgodę".
   The key must be in the OS credential store (keys made by the app are); otherwise the tab
   says to create a new key.
5. **The stdio bridge ships with the installers.** `scripts/release.mjs build` builds
   `lokalomat-mcp` and adds it to the bundle resources; the app finds it in its resource
   folder (or next to the executable). In development builds it is absent and only HTTP is offered.
6. The redaction rules also mask `lk-…` access keys.

## Consequences

- A program that is not allowed cannot start a session, and the user sees who asked.
- The name is self-declared and the later-request check is per key, not per program. The
  dialog stops programs the user did not intend to connect; it is not protection against a
  program that already holds a key (such a program can claim an allowed name, or call REST).
  The key stays the real secret, and revoking a key still cuts off everything.
- A client that was connected when its permission was withdrawn learns about it on its next
  call, not at once.

## Verification

- Core: the client store (remember, refresh, revoke, key revoke removes clients, names are
  cleaned). Gateway: a fake prompt is asked once per client, again after revoking, a "no" is
  not remembered, no prompt means open; over HTTP a refused client gets -32001 for everything
  but `ping`.
- UI (Playwright, mock backend): the question appears, blocks the page behind it, takes
  focus, has no axe violations and Escape refuses; *Allow* closes it; the MCP tab lists a
  program and withdraws it.
- `claude mcp add` was run for both shapes in a throwaway config folder (HTTP with a header,
  stdio with environment variables); Claude Code listed both and the bridge's "app not
  running" error reached its health check.
- Real app (release build on NIAGARA, driven through WebView2 CDP, with the real `lokalomat-mcp`
  bridge as the client): a new program made the dialog appear with its name and the key, the
  page behind was inert, and the `initialize` answer waited; *Zezwól* let it in and the nine
  tools worked; connecting again asked nothing; the list showed the program and its version;
  withdrawing it made `tools/list` fail with -32001; the next connection asked again, Escape
  refused it and nothing was remembered; the MCP tab showed the command with the key masked.
  "Dodaj do Claude Code" was not clicked in the real app (it would change the user's own
  Claude settings); its command shape was checked as described above.
- Not run: macOS, other machines, the installer with the bridge in its resources (the release
  script change is untested until the next release build).
