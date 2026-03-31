# Development

## Repository Setup

This repository vendors `codex` as a Git submodule and reuses Codex crates
through in-repo path dependencies.

Clone and initialize the repository with its submodule:

```bash
git clone --recursive <your-codex-proxy-repo>
cd codex-proxy
```

If you already cloned without submodules:

```bash
git submodule update --init --depth 1
```

## Run From Source

```bash
cargo run -- \
  --bind 127.0.0.1:8787 \
  --data-dir /tmp/codex-proxy \
  --admin-password 'your-console-password'
```

First web-console login uses the admin password. `admin key` is created later in
the web console when you need automation access.

Useful auth-related options:

```bash
--auth-issuer https://auth.openai.com
--auth-client-id app_EMoamEEZ73f0CkXaXp7hrann
--auth-callback-url http://localhost:1455/auth/callback
```

`--auth-callback-url` is the local callback URL that OpenAI redirects to after
browser auth. `codex-proxy` does not need to listen on that URL. The intended
flow is:

- finish login in a browser
- let the browser land on the callback URL
- copy the full callback URL from the address bar
- submit it through `/admin/auth/browser/:id/complete` or the Browser Auth
  import modal

To stay aligned with the official Codex browser login flow,
`--auth-callback-url` must remain an explicit loopback HTTP URL such as
`http://localhost:1455/auth/callback`.

## Frontend Console

`codex-proxy` ships with a Vue 3 admin console under `ui/`.

- The backend serves the built frontend from `/`
- Built assets are read from `ui/dist`
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

If you deploy frontend assets to a custom location, set:

```bash
export CODEX_PROXY_UI_DIST_DIR=/path/to/ui/dist
```
