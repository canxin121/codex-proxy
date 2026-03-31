# codex-proxy

`codex-proxy` is an independent Rust service for sharing many Codex ChatGPT auth credentials across devices.

## What It Does

- Separates proxy `api key` access from admin access.
- Uses `api key` for Codex clients only.
- Uses admin password for human web-console login.
- Uses `admin key` for automation scripts that need full admin API access.
- Lets you auth and manage Codex accounts from the web console.
- Load-balances across multiple credentials and records request stats and errors.

## Quick Start

Clone with the vendored `codex` submodule:

```bash
git clone --recursive <your-codex-proxy-repo>
cd codex-proxy
```

Install the latest release:

```bash
curl -fsSL https://raw.githubusercontent.com/canxin121/codex-proxy/main/scripts/install.sh \
  | bash -s -- -- \
      --bind 127.0.0.1:8787 \
      --data-dir "$HOME/.local/share/codex-proxy/data" \
      --admin-password 'change-me'
```

On Linux with `systemd --user` available, installation also creates and starts a
user-level service by default. It never creates a system-level service.

Or run from source:

```bash
cargo run -- \
  --bind 127.0.0.1:8787 \
  --data-dir /tmp/codex-proxy \
  --admin-password 'your-console-password'
```

First web-console login uses the admin password. After login, create:

- `admin key` for scripts and admin API automation
- `api key` for Codex clients that should use the proxy

## Docs

- [Install, update, uninstall](./docs/install.md)
- [Run from source and frontend development](./docs/development.md)
- [Admin API and important routes](./docs/admin-api.md)
- [Release process](./docs/release.md)
