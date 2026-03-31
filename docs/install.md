# Installation

## Install

For Linux/macOS (`x86_64`), install the latest GitHub Release and persist the
same runtime flags you would normally pass to `cargo run --`:

```bash
curl -fsSL https://raw.githubusercontent.com/canxin121/codex-proxy/main/scripts/install.sh \
  | bash -s -- -- \
      --bind 127.0.0.1:8787 \
      --data-dir "$HOME/.local/share/codex-proxy/data" \
      --admin-password 'change-me'
```

Linux installer behavior:

- Tries `x86_64-unknown-linux-musl` first.
- Falls back to `x86_64-unknown-linux-gnu` if musl is unavailable for that tag.
- On Linux with `systemd --user` available, creates and starts a user-level
  service by default.

Everything after the installer `--` separator is saved and reused whenever the
installed `codex-proxy` launcher runs.

Install a specific release tag:

```bash
curl -fsSL https://raw.githubusercontent.com/canxin121/codex-proxy/main/scripts/install.sh \
  | bash -s -- --version v0.1.2 -- \
      --bind 127.0.0.1:8787 \
      --data-dir "$HOME/.local/share/codex-proxy/data" \
      --admin-password 'change-me'
```

Force a specific target when needed:

```bash
curl -fsSL https://raw.githubusercontent.com/canxin121/codex-proxy/main/scripts/install.sh \
  | bash -s -- --target x86_64-unknown-linux-musl -- \
      --bind 127.0.0.1:8787 \
      --data-dir "$HOME/.local/share/codex-proxy/data" \
      --admin-password 'change-me'
```

Override install paths if needed:

```bash
curl -fsSL https://raw.githubusercontent.com/canxin121/codex-proxy/main/scripts/install.sh \
  | bash -s -- \
      --install-bin-dir /usr/local/bin \
      --install-share-dir /usr/local/share/codex-proxy \
      -- \
      --bind 127.0.0.1:8787 \
      --data-dir /usr/local/share/codex-proxy/data \
      --admin-password 'change-me'
```

Installed files default to:

- launcher: `~/.local/bin/codex-proxy`
- real binary: `~/.local/share/codex-proxy/bin/codex-proxy`
- frontend assets: `~/.local/share/codex-proxy/ui/dist`
- saved runtime args: `~/.local/share/codex-proxy/runtime-args.sh`

The installer only creates user-space files. It does not create a system-level
service.

If the Linux user manager is available, the default install flow enables and
starts:

```bash
systemctl --user status codex-proxy.service
```

To opt out of service creation and only install files:

```bash
curl -fsSL https://raw.githubusercontent.com/canxin121/codex-proxy/main/scripts/install.sh \
  | bash -s -- --service-mode none -- \
      --bind 127.0.0.1:8787 \
      --data-dir "$HOME/.local/share/codex-proxy/data" \
      --admin-password 'change-me'
```

Saved runtime args are stored on disk so the launcher can reuse them. That
includes secrets such as `--admin-password`. The script writes those files with
user-only permissions.

## Update

Update to the latest GitHub Release:

```bash
curl -fsSL https://raw.githubusercontent.com/canxin121/codex-proxy/main/scripts/update.sh \
  | bash -s --
```

Update to a specific release tag:

```bash
curl -fsSL https://raw.githubusercontent.com/canxin121/codex-proxy/main/scripts/update.sh \
  | bash -s -- --version v0.1.2
```

Replace the saved runtime args during update:

```bash
curl -fsSL https://raw.githubusercontent.com/canxin121/codex-proxy/main/scripts/update.sh \
  | bash -s -- --version v0.1.2 -- \
      --bind 127.0.0.1:8787 \
      --data-dir "$HOME/.local/share/codex-proxy/data" \
      --admin-password 'new-password'
```

If no runtime args are passed after `--`, `update.sh` reuses the existing saved
runtime args.

If a user-level service is installed, `update.sh` refreshes the files and
restarts that service.

If you need to change saved options such as `--bind`, `--data-dir`, or
`--admin-password`, rerun `update.sh` with a new runtime-args section instead
of passing duplicate flags to the launcher.

## Uninstall

Remove the installed binary and frontend assets:

```bash
curl -fsSL https://raw.githubusercontent.com/canxin121/codex-proxy/main/scripts/uninstall.sh \
  | bash -s --
```

Also remove the runtime data directory when the saved `--data-dir` can be
inferred:

```bash
curl -fsSL https://raw.githubusercontent.com/canxin121/codex-proxy/main/scripts/uninstall.sh \
  | bash -s -- --remove-data-dir
```

Or provide the data dir explicitly:

```bash
curl -fsSL https://raw.githubusercontent.com/canxin121/codex-proxy/main/scripts/uninstall.sh \
  | bash -s -- --remove-data-dir --data-dir /path/to/codex-proxy-data
```

If a user-level service exists, uninstall also stops, disables, and removes it.
