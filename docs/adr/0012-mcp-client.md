# ADR 0012: MCP servers (local and remote) as chat tools

- Status: accepted
- Date: 2026-11-11

## Context

The product owner asked for local (stdio) and remote (HTTP) MCP servers with a
configuration UI, with `mcp-grafana` as the reference server (milestone M2b, beyond the
PRD; planned as "ADR 0004" in the original plan).

## Decision

- Client: the official Rust SDK `rmcp` 3.9 – `TokioChildProcess` for stdio (stderr goes
  to `logs/mcp-<id>.log`, no console window on Windows) and Streamable HTTP via reqwest
  with native-tls (same TLS stack as the rest of the app). Legacy SSE-only servers are
  not supported.
- Storage: `mcp_servers` table (config JSON without secrets). Values marked secret
  (env vars, headers, bearer token) live in the OS credential store under
  `Studio Lokalsi MCP / <id>:<key>`; a blank secret field in the form keeps the stored
  value. Export/import uses the common `{"mcpServers": {...}}` format; imported names
  containing token/key/secret/password/authorization are marked secret, export never
  contains secret values.
- Lifecycle: servers connect when switched on, tested, or used by the chat; a closed
  connection is reconnected on the next use; failures put the card into an error state
  and the server is skipped in chat – never a crash.
- Chat: tools of enabled servers are offered to models whose catalog entry has the
  `tools` feature, named `<server slug>__<tool>`. Per-tool policy: ask every time
  (default), always allow, or block (not offered). Asking shows a consent card in the
  answer (Allow / Always allow / Don't allow). Results go back to the model as data; at
  most 6 tool rounds per answer, results cut at 16 000 characters (UI preview 4 000).
- Privacy: remote servers carry a "sends data off this computer" badge and a warning in
  the form and consent card. Tool arguments and results are logged only through
  `loggable_content` (hidden by default); secrets go through `redact` (e.g. `glsa_…`).

## Consequences

`mcp-grafana` is added by pointing at its executable (template fills `GRAFANA_URL` and a
secret `GRAFANA_SERVICE_ACCOUNT_TOKEN`); automatic download of MCP servers could follow
the plug-and-play rule later. OAuth for remote servers is not implemented yet.
