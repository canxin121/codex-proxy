#!/usr/bin/env bash
set -euo pipefail

REPO="${CODEX_PROXY_REPO:-canxin121/codex-proxy}"
VERSION="${CODEX_PROXY_VERSION:-}"
SCRIPT_REF="${CODEX_PROXY_SCRIPT_REF:-}"
TARGET_OVERRIDE="${CODEX_PROXY_TARGET:-}"
INSTALL_BIN_DIR="${CODEX_PROXY_INSTALL_BIN_DIR:-$HOME/.local/bin}"
INSTALL_SHARE_DIR="${CODEX_PROXY_INSTALL_SHARE_DIR:-$HOME/.local/share/codex-proxy}"
CLEANUP_TMP_DIR=""

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command not found: $1" >&2
    exit 1
  fi
}

cleanup_tmp_dir() {
  if [[ -n "${CLEANUP_TMP_DIR}" ]]; then
    rm -rf "${CLEANUP_TMP_DIR}"
  fi
}

resolve_script_ref() {
  if [[ -n "${SCRIPT_REF}" ]]; then
    printf '%s\n' "${SCRIPT_REF}"
    return 0
  fi

  if [[ -n "${VERSION}" ]]; then
    printf '%s\n' "${VERSION}"
    return 0
  fi

  printf 'main\n'
}

main() {
  local script_ref tmp_dir install_script_url

  need_cmd curl
  need_cmd mktemp

  script_ref="$(resolve_script_ref)"
  install_script_url="https://raw.githubusercontent.com/${REPO}/${script_ref}/scripts/install.sh"

  tmp_dir="$(mktemp -d)"
  CLEANUP_TMP_DIR="${tmp_dir}"
  trap cleanup_tmp_dir EXIT

  echo "Updating codex-proxy using ${install_script_url} ..."

  curl -fsSL "${install_script_url}" \
    | CODEX_PROXY_REPO="${REPO}" \
      CODEX_PROXY_VERSION="${VERSION}" \
      CODEX_PROXY_TARGET="${TARGET_OVERRIDE}" \
      CODEX_PROXY_INSTALL_BIN_DIR="${INSTALL_BIN_DIR}" \
      CODEX_PROXY_INSTALL_SHARE_DIR="${INSTALL_SHARE_DIR}" \
      bash
}

main "$@"
