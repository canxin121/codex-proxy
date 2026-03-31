#!/usr/bin/env bash
set -euo pipefail

REPO="canxin121/codex-proxy"
VERSION=""
SCRIPT_REF=""
TARGET_OVERRIDE=""
INSTALL_BIN_DIR="${HOME}/.local/bin"
INSTALL_SHARE_DIR="${HOME}/.local/share/codex-proxy"
RUNTIME_ARGS=()
RUNTIME_ARGS_EXPLICIT=0
CLEANUP_TMP_DIR=""

REPO_EXPLICIT=0
TARGET_EXPLICIT=0
INSTALL_BIN_DIR_EXPLICIT=0
INSTALL_SHARE_DIR_EXPLICIT=0

usage() {
  cat <<'EOF'
Usage:
  update.sh [installer-options] [-- codex-proxy-runtime-args...]

Installer options:
  --repo <owner/name>          GitHub repository to download from
  --version <tag>              Release tag to install; omit to update to latest
  --script-ref <ref>           Git ref used to fetch install.sh (defaults to tag, else main)
  --target <triple>            Force a specific release target
  --install-bin-dir <path>     Directory for the user-facing launcher
  --install-share-dir <path>   Directory for shared files and the real binary
  -h, --help                   Show this help

If no runtime args are passed after `--`, the previously installed runtime args
are reused from the local installation metadata.

Example:
  curl -fsSL https://raw.githubusercontent.com/canxin121/codex-proxy/main/scripts/update.sh \
    | bash -s -- --version v0.1.0
EOF
}

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
      --repo)
        require_option_value "$1" "${2:-}"
        REPO="$2"
        REPO_EXPLICIT=1
        shift 2
        ;;
      --repo=*)
        REPO="${1#*=}"
        REPO_EXPLICIT=1
        shift
        ;;
      --version)
        require_option_value "$1" "${2:-}"
        VERSION="$2"
        shift 2
        ;;
      --version=*)
        VERSION="${1#*=}"
        shift
        ;;
      --script-ref)
        require_option_value "$1" "${2:-}"
        SCRIPT_REF="$2"
        shift 2
        ;;
      --script-ref=*)
        SCRIPT_REF="${1#*=}"
        shift
        ;;
      --target)
        require_option_value "$1" "${2:-}"
        TARGET_OVERRIDE="$2"
        TARGET_EXPLICIT=1
        shift 2
        ;;
      --target=*)
        TARGET_OVERRIDE="${1#*=}"
        TARGET_EXPLICIT=1
        shift
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
      --)
        shift
        RUNTIME_ARGS=("$@")
        RUNTIME_ARGS_EXPLICIT=1
        return 0
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
  local metadata_path runtime_args_path
  metadata_path="${INSTALL_SHARE_DIR}/install-metadata.env"
  if [[ ! -f "${metadata_path}" ]]; then
    return 0
  fi

  # shellcheck disable=SC1090
  . "${metadata_path}"

  if [[ "${REPO_EXPLICIT}" -eq 0 ]]; then
    REPO="${CODEX_PROXY_METADATA_REPO:-${CODEX_PROXY_REPO:-${REPO}}}"
  fi
  if [[ "${TARGET_EXPLICIT}" -eq 0 ]]; then
    TARGET_OVERRIDE="${CODEX_PROXY_METADATA_TARGET:-${CODEX_PROXY_TARGET:-${TARGET_OVERRIDE}}}"
  fi
  if [[ "${INSTALL_BIN_DIR_EXPLICIT}" -eq 0 ]]; then
    INSTALL_BIN_DIR="${CODEX_PROXY_METADATA_INSTALL_BIN_DIR:-${CODEX_PROXY_INSTALL_BIN_DIR:-${INSTALL_BIN_DIR}}}"
  fi
  if [[ "${INSTALL_SHARE_DIR_EXPLICIT}" -eq 0 ]]; then
    INSTALL_SHARE_DIR="${CODEX_PROXY_METADATA_INSTALL_SHARE_DIR:-${CODEX_PROXY_INSTALL_SHARE_DIR:-${INSTALL_SHARE_DIR}}}"
  fi

  runtime_args_path="${CODEX_PROXY_METADATA_RUNTIME_ARGS_FILE:-${INSTALL_SHARE_DIR}/runtime-args.sh}"
  if [[ "${RUNTIME_ARGS_EXPLICIT}" -eq 0 && -f "${runtime_args_path}" ]]; then
    unset CODEX_PROXY_RUNTIME_ARGS || true
    # shellcheck disable=SC1090
    . "${runtime_args_path}"
    if declare -p CODEX_PROXY_RUNTIME_ARGS >/dev/null 2>&1; then
      RUNTIME_ARGS=("${CODEX_PROXY_RUNTIME_ARGS[@]}")
    fi
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

print_runtime_args() {
  local -a rendered
  local index arg next
  local label

  if [[ "${RUNTIME_ARGS_EXPLICIT}" -eq 1 ]]; then
    label="Using runtime args:"
  else
    label="Reusing runtime args:"
  fi

  if [[ "${#RUNTIME_ARGS[@]}" -eq 0 ]]; then
    echo "${label} (none)"
    return 0
  fi

  rendered=()
  index=0
  while [[ "${index}" -lt "${#RUNTIME_ARGS[@]}" ]]; do
    arg="${RUNTIME_ARGS[${index}]}"
    case "${arg}" in
      --admin-password|--database-url)
        rendered+=("${arg}")
        next="<missing>"
        if [[ $((index + 1)) -lt "${#RUNTIME_ARGS[@]}" ]]; then
          next="<redacted>"
          index=$((index + 2))
        else
          index=$((index + 1))
        fi
        rendered+=("${next}")
        ;;
      --admin-password=*|--database-url=*)
        rendered+=("${arg%%=*}=<redacted>")
        index=$((index + 1))
        ;;
      *)
        rendered+=("${arg}")
        index=$((index + 1))
        ;;
    esac
  done

  printf '%s' "${label}"
  printf ' %q' "${rendered[@]}"
  printf '\n'
}

main() {
  local script_ref install_script_url
  local -a bash_cmd

  parse_args "$@"
  load_existing_metadata

  need_cmd curl

  script_ref="$(resolve_script_ref)"
  install_script_url="https://raw.githubusercontent.com/${REPO}/${script_ref}/scripts/install.sh"

  bash_cmd=(
    bash -s --
    --repo "${REPO}"
    --install-bin-dir "${INSTALL_BIN_DIR}"
    --install-share-dir "${INSTALL_SHARE_DIR}"
  )

  if [[ -n "${VERSION}" ]]; then
    bash_cmd+=(--version "${VERSION}")
  fi
  if [[ -n "${TARGET_OVERRIDE}" ]]; then
    bash_cmd+=(--target "${TARGET_OVERRIDE}")
  fi
  if [[ "${#RUNTIME_ARGS[@]}" -gt 0 ]]; then
    bash_cmd+=(-- "${RUNTIME_ARGS[@]}")
  fi

  echo "Updating codex-proxy using ${install_script_url} ..."
  print_runtime_args

  curl -fsSL "${install_script_url}" | "${bash_cmd[@]}"
}

main "$@"
