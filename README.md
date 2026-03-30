# codex-proxy

`codex-proxy` is an independent Rust service for sharing many Codex ChatGPT auth credentials across devices.

It vendors `codex` as a Git submodule and reuses Codex crates through in-repo path dependencies:

- `codex-login::AuthManager` for per-credential auth storage and refresh.
- `codex-login::request_device_code` and `codex-login::complete_device_code_login` for device-code auth.
- Codex's ChatGPT upstream base URL default: `https://chatgpt.com/backend-api/codex`
- `codex-api::rate_limits` for parsing rate-limit headers and websocket events.

Repository layout:

```text
codex-proxy/
  Cargo.toml
  src/
  ui/
  vendor/
    codex/
```

`Cargo.toml` points to `vendor/codex/codex-rs/...`, so `vendor/codex` must be initialized before building.

Clone and initialize the repository with its submodule:

```bash
git clone --recursive <your-codex-proxy-repo>
cd codex-proxy
```

If you already cloned without submodules:

```bash
git submodule update --init --depth 1
```

## Frontend console

`codex-proxy` also ships with a Vue 3 admin console under `ui/`.

- The backend serves the built frontend from `/`
- Built assets are read from `codex-proxy/ui/dist`
- API routes stay on the same origin under `/admin/...`

For frontend development:

```bash
cd ui
pnpm install
pnpm dev
```

For a production build:

```bash
cd ui
pnpm build
```

Then run `codex-proxy`; opening the root URL will show the admin console.

## What it does

- Stores many ChatGPT-backed Codex credentials.
- Generates and validates proxy-side API keys for your own devices and clients.
- Supports two admin auth flows:
  - browser auth via manual callback URL submission
  - device-code auth via background completion
- Selects a credential per request using:
  - stored rate-limit snapshots
  - current in-flight request count
  - credential weight
  - recent failure count
- Persists per-request records with:
  - credential and API key attribution
  - success and failure timestamps
  - status code, error phase, error code, and error message
  - response id and requested model
  - input, cached-input, output, reasoning-output, and total token usage
- Exposes aggregated stats and latest-request-error snapshots for:
  - each credential
  - each API key
  - overall proxy traffic
- Exposes richer usage analytics for the admin UI, including:
  - day/hour traffic trends
  - token trends
  - credential, API key, model, path, status-code, and error-phase breakdowns
- Proxies:
  - HTTP `POST /responses`
  - HTTP `POST /responses/compact`
  - HTTP `GET /models`
  - WebSocket `GET /responses`

Both `/responses` and `/v1/responses` style paths are supported.

## Run

```bash
cargo run -- \
  --bind 127.0.0.1:8787 \
  --data-dir /tmp/codex-proxy
```

Useful auth-related options:

```bash
--auth-issuer https://auth.openai.com
--auth-client-id app_EMoamEEZ73f0CkXaXp7hrann
--auth-callback-url http://localhost:1455/auth/callback
```

`--auth-callback-url` is the URL that OpenAI redirects to after browser auth. `codex-proxy` does not have to listen on that URL. The intended flow is: finish login in a browser, let the browser land on that callback URL, then copy the full callback URL from the address bar and send it to the backend through `/admin/auth/browser/:id/complete`.

If `CODEX_PROXY_ADMIN_TOKEN` is not set, the service generates one at startup and prints it to logs.

## Important routes

- `GET /healthz`
- `GET /readyz`
- `GET /`
- `GET /admin/credentials`
- `POST /admin/credentials`
- `GET /admin/credentials/:id`
- `PATCH /admin/credentials/:id`
- `DELETE /admin/credentials/:id`
- `POST /admin/credentials/:id/refresh`
- `GET /admin/auth/sessions`
- `GET /admin/auth/sessions/:id`
- `POST /admin/auth/sessions/:id/cancel`
- `POST /admin/auth/browser`
- `POST /admin/auth/browser/:id/complete`
- `POST /admin/auth/device-code`
- `GET /admin/api-keys`
- `POST /admin/api-keys`
- `GET /admin/api-keys/:id`
- `PATCH /admin/api-keys/:id`
- `DELETE /admin/api-keys/:id`
- `GET /admin/stats/overview`
- `GET /admin/stats/usage`
- `GET /admin/stats/requests`

Admin routes require:

```http
Authorization: Bearer <admin-token>
```

Proxy routes require either the admin token or a generated proxy API key.

`GET /admin/credentials` and `GET /admin/api-keys` now include:

- `request_stats`
- `last_request_error`

The overview route returns global counters plus recent failures, and the request-record route supports these query parameters:

- `limit`
- `credential_id`
- `api_key_id`
- `only_failures`

The usage analytics route supports:

- `credential_id`
- `api_key_id`
- `only_failures`
- `top`

## Credential payload

Create a credential record:

```json
{
  "credential_name": "workspace-a",
  "load_balance_weight": 1,
  "is_enabled": true,
  "credential_notes": "team alpha",
  "upstream_base_url": "https://chatgpt.com/backend-api/codex"
}
```

This only creates the record. Actual auth is done separately through `/admin/auth/...`.

## Browser auth flow

Start browser auth for an existing credential:

```json
POST /admin/auth/browser
{
  "credential_id": "..."
}
```

The response contains:

- `auth_session_id`
- `authorization_url`
- `auth_redirect_url`

Then:

1. Open `authorization_url` in a browser.
2. Finish sign-in.
3. Let the browser redirect to `auth_redirect_url`.
4. Copy the full callback URL from the browser address bar.
5. Submit it back:

```json
POST /admin/auth/browser/:auth_session_id/complete
{
  "callback_url": "http://localhost:1455/auth/callback?code=...&state=..."
}
```

## Device-code auth flow

Start device-code auth:

```json
POST /admin/auth/device-code
{
  "credential_id": "..."
}
```

The response contains:

- `auth_session_id`
- `verification_url`
- `user_code`

After the user enters the code, the backend completes auth in the background. Poll the session with:

```http
GET /admin/auth/sessions/:auth_session_id
```

## Proxy API key example

Create a proxy API key:

```json
{
  "api_key_name": "laptop",
  "has_admin_access": false,
  "api_key_expires_at": null
}
```

The plain text proxy API key is returned only once in `api_key_value`.
