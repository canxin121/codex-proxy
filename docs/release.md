# Release Process

## GitHub Actions

This repository includes these GitHub Actions workflows:

- CI: `.github/workflows/ci.yml`
  - `cargo fmt --check`
  - `cargo check --locked`
  - `cargo test --locked`
  - `ui` build with `pnpm build`
- Release: `.github/workflows/release.yml`
  - triggers on tags matching `v*`
  - also supports manual `workflow_dispatch`
  - builds release archives and publishes a GitHub Release
  - publishes both Linux musl and Linux gnu artifacts, with musl as the
    primary distribution target
- Post-release verification: `.github/workflows/release-install-test.yml`
  - triggers on `release.published`
  - also runs after the `Release` workflow completes successfully
  - downloads tagged install scripts from GitHub
  - runs real install, update, smoke-run, and uninstall flows

## Create Release 0.1.3

```bash
git tag v0.1.3
git push origin v0.1.3
```

You can also use the `Release` workflow manually with `tag=v0.1.3`.

## Asset Names

Release assets follow this format:

- `codex-proxy-<tag>-x86_64-unknown-linux-musl.tar.gz`
- `codex-proxy-<tag>-x86_64-unknown-linux-gnu.tar.gz`
- `codex-proxy-<tag>-x86_64-apple-darwin.tar.gz`
- `codex-proxy-<tag>-x86_64-pc-windows-msvc.zip`
