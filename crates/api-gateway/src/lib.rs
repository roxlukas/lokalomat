//! Local API gateway (PRD §4.2, §15 item 12; ADR 0020).
//!
//! M5 scope: only what the Android client needs. The gateway listens on the
//! local network only when the user enables it, over TLS with a self-signed
//! certificate the phone pins from the pairing QR code. Every request needs a
//! per-device access key issued after the user confirms the comparison code;
//! requests from browsers (with an `Origin` header) are refused.
//!
//! * [`api`] – request/response types shared by the desktop and the phone,
//! * [`pairing`] – QR payload, comparison code, device identity,
//! * [`tls`] – certificate fingerprint and the pinning verifier,
//! * `server` / `backend` / `thumbs` – the gateway in the desktop app (feature `server`),
//! * `openai` – the local OpenAI-compatible server for agents (M5c, ADR 0025),
//! * `mcp_server` – the same server as an MCP server (M7, ADR 0033),
//! * `client` – the phone side (feature `client`).

pub mod api;
pub mod pairing;
pub mod tls;

#[cfg(feature = "server")]
pub mod backend;
#[cfg(feature = "server")]
pub mod keys;
#[cfg(feature = "server")]
pub mod openai;
#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "client")]
pub mod client;

/// Default port of the LAN gateway (changeable in the settings).
pub const DEFAULT_LAN_PORT: u16 = 47_821;
