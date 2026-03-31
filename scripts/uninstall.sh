#!/usr/bin/env bash
set -euo pipefail

INSTALL_BIN_DIR="${HOME}/.local/bin"
INSTALL_SHARE_DIR="${HOME}/.local/share/codex-proxy"
REMOVE_DATA_DIR=0
DATA_DIR=""

INSTALL_BIN_DIR_EXPLICIT=0
INSTALL_SHARE_DIR_EXPLICIT=0

usage() {
  cat <<'EOF'
Usage:
  uninstall.sh [options]

Options:
  --install-bin-dir <path>     Directory containing the launcher
  --install-share-dir <path>   Directory containing shared files
  --remove-data-dir            Also remove the runtime data directory
  --data-dir <path>            Explicit data directory to remove
  -h, --help                   Show this help

If `--remove-data-dir` is used without `--data-dir`, the script tries to infer
the saved `--data-dir` value from the installed runtime args file.
EOF
}

require_option_value() {
  local option="$1"
  local value="${2:-}"
  if [[ -z "${value}" ]]; then
    echo "error: ${option} requires a value" >&2
    exit 1
  fi
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      -h|--help)
        usage
        exit 0
        ;;
      --install-bin-dir)
        require_option_value "$1" "${2:-}"
        INSTALL_BIN_DIR="$2"
        INSTALL_BIN_DIR_EXPLICIT=1
        shift 2
        ;;
      --install-bin-dir=*)
        INSTALL_BIN_DIR="${1#*=}"
        INSTALL_BIN_DIR_EXPLICIT=1
        shift
        ;;
      --install-share-dir)
        require_option_value "$1" "${2:-}"
        INSTALL_SHARE_DIR="$2"
        INSTALL_SHARE_DIR_EXPLICIT=1
        shift 2
        ;;
      --install-share-dir=*)
        INSTALL_SHARE_DIR="${1#*=}"
        INSTALL_SHARE_DIR_EXPLICIT=1
        shift
        ;;
      --remove-data-dir)
        REMOVE_DATA_DIR=1
        shift
        ;;
      --data-dir)
        require_option_value "$1" "${2:-}"
        DATA_DIR="$2"
        shift 2
        ;;
      --data-dir=*)
        DATA_DIR="${1#*=}"
        shift
        ;;
      *)
        echo "error: unknown option: $1" >&2
        echo >&2
        usage >&2
        exit 1
        ;;
    esac
  done
}

load_existing_metadata() {
  local metadata_path
  metadata_path="${INSTALL_SHARE_DIR}/install-metadata.env"
  if [[ ! -f "${metadata_path}" ]]; then
    return 0
  fi

  # shellcheck disable=SC1090
  . "${metadata_path}"

  if [[ "${INSTALL_BIN_DIR_EXPLICIT}" -eq 0 ]]; then
    INSTALL_BIN_DIR="${CODEX_PROXY_METADATA_INSTALL_BIN_DIR:-${CODEX_PROXY_INSTALL_BIN_DIR:-${INSTALL_BIN_DIR}}}"
  fi
  if [[ "${INSTALL_SHARE_DIR_EXPLICIT}" -eq 0 ]]; then
    INSTALL_SHARE_DIR="${CODEX_PROXY_METADATA_INSTALL_SHARE_DIR:-${CODEX_PROXY_INSTALL_SHARE_DIR:-${INSTALL_SHARE_DIR}}}"
  fi
}

extract_data_dir_from_runtime_args() {
  local runtime_args_path arg
  runtime_args_path="${INSTALL_SHARE_DIR}/runtime-args.sh"
  if [[ ! -f "${runtime_args_path}" ]]; then
    return 0
  fi

  unset CODEX_PROXY_RUNTIME_ARGS || true
  # shellcheck disable=SC1090
  . "${runtime_args_path}"
  if ! declare -p CODEX_PROXY_RUNTIME_ARGS >/dev/null 2>&1; then
    return 0
  fi

  while [[ "${#CODEX_PROXY_RUNTIME_ARGS[@]}" -gt 0 ]]; do
    arg="${CODEX_PROXY_RUNTIME_ARGS[0]}"
    case "${arg}" in
      --data-dir)
        if [[ "${#CODEX_PROXY_RUNTIME_ARGS[@]}" -ge 2 ]]; then
          printf '%s\n' "${CODEX_PROXY_RUNTIME_ARGS[1]}"
          return 0
        fi
        return 0
        ;;
      --data-dir=*)
        printf '%s\n' "${arg#*=}"
        return 0
        ;;
    esac
    CODEX_PROXY_RUNTIME_ARGS=("${CODEX_PROXY_RUNTIME_ARGS[@]:1}")
  done
}

main() {
  local launcher_path removed_any inferred_data_dir
  removed_any=0

  parse_args "$@"
  load_existing_metadata

  launcher_path="${INSTALL_BIN_DIR}/codex-proxy"

  if [[ -e "${launcher_path}" ]]; then
    rm -f "${launcher_path}"
    echo "Removed launcher: ${launcher_path}"
    removed_any=1
  fi

  if [[ "${REMOVE_DATA_DIR}" -eq 1 && -z "${DATA_DIR}" ]]; then
    inferred_data_dir="$(extract_data_dir_from_runtime_args || true)"
    if [[ -n "${inferred_data_dir}" ]]; then
      DATA_DIR="${inferred_data_dir}"
    fi
  fi

  if [[ -d "${INSTALL_SHARE_DIR}" ]]; then
    rm -rf "${INSTALL_SHARE_DIR}"
    echo "Removed shared files: ${INSTALL_SHARE_DIR}"
    removed_any=1
  fi

  if [[ "${REMOVE_DATA_DIR}" -eq 1 ]]; then
    if [[ -z "${DATA_DIR}" ]]; then
      echo "error: --data-dir is required when --remove-data-dir is used and no saved --data-dir could be inferred" >&2
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

  if [[ "${REMOVE_DATA_DIR}" -ne 1 ]]; then
    echo "Data directories were left untouched. Use --remove-data-dir to remove them."
  fi
}

main "$@"
