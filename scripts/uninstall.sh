#!/usr/bin/env bash
set -euo pipefail

INSTALL_BIN_DIR="${CODEX_PROXY_INSTALL_BIN_DIR:-$HOME/.local/bin}"
INSTALL_SHARE_DIR="${CODEX_PROXY_INSTALL_SHARE_DIR:-$HOME/.local/share/codex-proxy}"
REMOVE_DATA_DIR="${CODEX_PROXY_REMOVE_DATA_DIR:-0}"
DATA_DIR="${CODEX_PROXY_DATA_DIR:-}"

main() {
  local bin_path removed_any
  removed_any=0
  bin_path="${INSTALL_BIN_DIR}/codex-proxy"

  if [[ -e "${bin_path}" ]]; then
    rm -f "${bin_path}"
    echo "Removed binary: ${bin_path}"
    removed_any=1
  fi

  if [[ -d "${INSTALL_SHARE_DIR}" ]]; then
    rm -rf "${INSTALL_SHARE_DIR}"
    echo "Removed shared files: ${INSTALL_SHARE_DIR}"
    removed_any=1
  fi

  if [[ "${REMOVE_DATA_DIR}" == "1" ]]; then
    if [[ -z "${DATA_DIR}" ]]; then
      echo "error: CODEX_PROXY_DATA_DIR is required when CODEX_PROXY_REMOVE_DATA_DIR=1" >&2
      exit 1
    fi
    if [[ -e "${DATA_DIR}" ]]; then
      rm -rf "${DATA_DIR}"
      echo "Removed data dir: ${DATA_DIR}"
      removed_any=1
    fi
  fi

  if [[ "${removed_any}" -eq 0 ]]; then
    echo "No codex-proxy installation found under ${INSTALL_BIN_DIR} and ${INSTALL_SHARE_DIR}."
  else
    echo "Uninstalled codex-proxy."
  fi

  if [[ "${REMOVE_DATA_DIR}" != "1" ]]; then
    echo "Data directories are not removed automatically. Set CODEX_PROXY_REMOVE_DATA_DIR=1 and CODEX_PROXY_DATA_DIR=/path if needed."
  fi
}

main "$@"
