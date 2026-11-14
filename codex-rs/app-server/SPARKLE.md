# Sparkle app-server connection

The optional connection lets the existing app-server connect to a local Sparkle
WebSocket endpoint. It does not start a separate app-server. Both standalone
and embedded app-servers use the same setting.

At startup, the app-server uses Codex's existing configuration, including the
optional setting in `$CODEX_HOME/config.toml` (normally `~/.codex/config.toml`):

```toml
web_server_port = 38471
```

The setting is optional; omitting it disables the connection. It uses the
existing configuration layers and command-line overrides. Changes take effect
when the app-server restarts. The connection code never writes this file.

The app-server connects directly to `127.0.0.1` on the configured port using
`/codex/app-server`, without system or environment proxy settings. If Sparkle is not
running or the connection closes, the app-server retries in the background
with exponential backoff from 2 to 30 seconds. Connection failure does not
block the terminal or an existing session.

The WebSocket upgrade request contains:

- `x-codex-pid`: process ID.
- `x-codex-instance-id`: unique app-server instance ID, stable across reconnects.
- `x-codex-runtime`: `embedded`, `shared`, or `standalone`.

Sparkle accepts the WebSocket and acts as the **app-server protocol client** on
that connection, even though the app-server initiated the WebSocket. Start with
the standard `initialize` request, then send `initialized` after receiving its
response. For example:

```json
{"id":1,"method":"initialize","params":{"clientInfo":{"name":"codex-backend","version":"1.0.0"},"capabilities":{"experimentalApi":true,"requiredHumanApproval":true}}}
```

```json
{"method":"initialized"}
```

The `codex-backend` client name uses the existing non-originating-client path,
so attaching the gateway does not replace the process-wide client User-Agent.
Each WebSocket text frame contains one app-server JSON message; app-server does
not require a `jsonrpc` field. Use `thread/loaded/list` to find threads owned by
this instance, then `thread/resume` to subscribe to an existing thread. Pending
ordinary approval requests are replayed on subscription. After reconnecting,
initialize and subscribe again to restore state. The protocol can be generated
with `codex app-server generate-ts --experimental`.

A newly created thread may not have a rollout until its first message. In that
case, `thread/resume` can return `no rollout found`; retry once the thread has
started a turn.

This connection exposes only existing app-server protocol capabilities. It does
not add an RPC for exiting the terminal or exposing the custom `tk/s` value.
