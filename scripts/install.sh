#!/usr/bin/env bash
set -euo pipefail

REPO="canxin121/codex-proxy"
VERSION=""
TARGET_OVERRIDE=""
INSTALL_BIN_DIR="${HOME}/.local/bin"
INSTALL_SHARE_DIR="${HOME}/.local/share/codex-proxy"
RUNTIME_ARGS=()
SERVICE_MODE="auto"
SERVICE_NAME="codex-proxy"
SERVICE_MANAGER="none"
SERVICE_UNIT_PATH=""
CLEANUP_TMP_DIR=""

usage() {
  cat <<'EOF'
Usage:
  install.sh [installer-options] [-- codex-proxy-runtime-args...]

Installer options:
  --repo <owner/name>          GitHub repository to download from
  --version <tag>              Release tag to install, for example v0.1.3
  --target <triple>            Force a specific release target
  --install-bin-dir <path>     Directory for the user-facing launcher
  --install-share-dir <path>   Directory for shared files and the real binary
  --service-mode <mode>        auto, user, or none
  --service-name <name>        User-service base name (default: codex-proxy)
  -h, --help                   Show this help

Everything after `--` is persisted and used as default arguments whenever the
installed `codex-proxy` launcher runs.

Example:
  curl -fsSL https://raw.githubusercontent.com/canxin121/codex-proxy/main/scripts/install.sh \
    | bash -s -- \
        --version v0.1.3 \
        --install-bin-dir "$HOME/.local/bin" \
        --install-share-dir "$HOME/.local/share/codex-proxy" \
        -- \
        --bind 127.0.0.1:8787 \
        --data-dir "$HOME/.local/share/codex-proxy/data" \
        --admin-password 'change-me'
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

normalize_service_name() {
  local raw_name="$1"
  if [[ "${raw_name}" == *.service ]]; then
    raw_name="${raw_name%.service}"
  fi
  if [[ -z "${raw_name}" || ! "${raw_name}" =~ ^[A-Za-z0-9_.@-]+$ ]]; then
    echo "error: invalid service name: ${raw_name}" >&2
    exit 1
  fi
  printf '%s\n' "${raw_name}"
}

normalize_service_mode() {
  local raw_mode="$1"
  case "${raw_mode}" in
    auto|user|none)
      printf '%s\n' "${raw_mode}"
      ;;
    *)
      echo "error: invalid --service-mode value: ${raw_mode}" >&2
      echo "expected one of: auto, user, none" >&2
      exit 1
      ;;
  esac
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
        shift 2
        ;;
      --repo=*)
        REPO="${1#*=}"
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
      --target)
        require_option_value "$1" "${2:-}"
        TARGET_OVERRIDE="$2"
        shift 2
        ;;
      --target=*)
        TARGET_OVERRIDE="${1#*=}"
        shift
        ;;
      --install-bin-dir)
        require_option_value "$1" "${2:-}"
        INSTALL_BIN_DIR="$2"
        shift 2
        ;;
      --install-bin-dir=*)
        INSTALL_BIN_DIR="${1#*=}"
        shift
        ;;
      --install-share-dir)
        require_option_value "$1" "${2:-}"
        INSTALL_SHARE_DIR="$2"
        shift 2
        ;;
      --install-share-dir=*)
        INSTALL_SHARE_DIR="${1#*=}"
        shift
        ;;
      --service-mode)
        require_option_value "$1" "${2:-}"
        SERVICE_MODE="$(normalize_service_mode "$2")"
        shift 2
        ;;
      --service-mode=*)
        SERVICE_MODE="$(normalize_service_mode "${1#*=}")"
        shift
        ;;
      --service-name)
        require_option_value "$1" "${2:-}"
        SERVICE_NAME="$(normalize_service_name "$2")"
        shift 2
        ;;
      --service-name=*)
        SERVICE_NAME="$(normalize_service_name "${1#*=}")"
        shift
        ;;
      --)
        shift
        RUNTIME_ARGS=("$@")
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

detect_target_candidates() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "${os}" in
    Linux)
      case "${arch}" in
        x86_64|amd64)
          echo "x86_64-unknown-linux-musl"
          echo "x86_64-unknown-linux-gnu"
          ;;
        *)
          echo "error: unsupported Linux architecture: ${arch}" >&2
          echo "supported: x86_64" >&2
          exit 1
          ;;
      esac
      ;;
    Darwin)
      case "${arch}" in
        x86_64|amd64)
          echo "x86_64-apple-darwin"
          ;;
        *)
          echo "error: unsupported macOS architecture: ${arch}" >&2
          echo "supported: x86_64" >&2
          exit 1
          ;;
      esac
      ;;
    *)
      echo "error: unsupported operating system: ${os}" >&2
      echo "this installer currently supports Linux and macOS." >&2
      exit 1
      ;;
  esac
}

resolve_latest_version() {
  local api_url
  api_url="https://api.github.com/repos/${REPO}/releases/latest"
  curl -fsSL "${api_url}" | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1
}

download_archive() {
  local version ext tmp_dir target archive_name download_url
  version="$1"
  ext="$2"
  tmp_dir="$3"
  shift 3

  for target in "$@"; do
    archive_name="codex-proxy-${version}-${target}.${ext}"
    download_url="https://github.com/${REPO}/releases/download/${version}/${archive_name}"
    echo "Trying ${archive_name} ..." >&2
    if curl -fsSL "${download_url}" -o "${tmp_dir}/${archive_name}"; then
      printf '%s\t%s\n' "${target}" "${archive_name}"
      return 0
    fi
    rm -f "${tmp_dir}/${archive_name}"
  done

  return 1
}

systemd_user_dir() {
  printf '%s\n' "${XDG_CONFIG_HOME:-${HOME}/.config}/systemd/user"
}

systemd_user_unit_name() {
  printf '%s.service\n' "${SERVICE_NAME}"
}

is_systemd_user_available() {
  if [[ "$(uname -s)" != "Linux" ]]; then
    return 1
  fi
  if ! command -v systemctl >/dev/null 2>&1; then
    return 1
  fi
  systemctl --user is-active default.target >/dev/null 2>&1
}

stop_existing_user_service_if_running() {
  local unit_name

  if [[ "${SERVICE_MODE}" == "none" ]]; then
    return 0
  fi

  if ! is_systemd_user_available; then
    return 0
  fi

  unit_name="$(systemd_user_unit_name)"
  if systemctl --user is-active --quiet "${unit_name}"; then
    systemctl --user stop "${unit_name}" >/dev/null
    echo "  user service stopped before replacing files: ${unit_name}" >&2
  fi
}

write_runtime_args() {
  local runtime_args_path="$1"
  shift

  {
    echo "CODEX_PROXY_RUNTIME_ARGS=("
    for arg in "$@"; do
      printf '  %q\n' "${arg}"
    done
    echo ")"
  } > "${runtime_args_path}"

  chmod 600 "${runtime_args_path}"
}

write_launcher_script() {
  local wrapper_path="$1"
  local real_bin_path="$2"
  local runtime_args_path="$3"
  local ui_dist_path="$4"

  cat > "${wrapper_path}" <<EOF
#!/usr/bin/env bash
set -euo pipefail

CODEX_PROXY_REAL_BIN_PATH=$(printf '%q' "${real_bin_path}")
CODEX_PROXY_RUNTIME_ARGS_FILE=$(printf '%q' "${runtime_args_path}")
CODEX_PROXY_UI_DIST=$(printf '%q' "${ui_dist_path}")

declare -a CODEX_PROXY_RUNTIME_ARGS=()
if [[ -f "\${CODEX_PROXY_RUNTIME_ARGS_FILE}" ]]; then
  # shellcheck disable=SC1090
  . "\${CODEX_PROXY_RUNTIME_ARGS_FILE}"
fi

export CODEX_PROXY_UI_DIST_DIR="\${CODEX_PROXY_UI_DIST}"
exec "\${CODEX_PROXY_REAL_BIN_PATH}" "\${CODEX_PROXY_RUNTIME_ARGS[@]}" "\$@"
EOF

  chmod +x "${wrapper_path}"
}

write_systemd_user_service() {
  local unit_path="$1"
  local wrapper_path="$2"

  cat > "${unit_path}" <<EOF
[Unit]
Description=codex-proxy user service
After=default.target network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=${wrapper_path}
Restart=always
RestartSec=2
WorkingDirectory=${HOME}

[Install]
WantedBy=default.target
EOF

  chmod 644 "${unit_path}"
}

install_user_service() {
  local wrapper_path="$1"
  local unit_dir unit_name unit_path linger_status

  if [[ "${SERVICE_MODE}" == "none" ]]; then
    return 0
  fi

  if [[ "${#RUNTIME_ARGS[@]}" -eq 0 ]]; then
    if [[ "${SERVICE_MODE}" == "user" ]]; then
      echo "error: cannot create a user service without saved runtime args" >&2
      exit 1
    fi
    echo "  user service: skipped (no saved runtime args)" >&2
    return 0
  fi

  if [[ "$(uname -s)" != "Linux" ]]; then
    if [[ "${SERVICE_MODE}" == "user" ]]; then
      echo "error: user-service installation is currently supported on Linux with systemd --user" >&2
      exit 1
    fi
    echo "  user service: skipped (automatic user service is currently supported on Linux with systemd --user)" >&2
    return 0
  fi

  if ! is_systemd_user_available; then
    if [[ "${SERVICE_MODE}" == "user" ]]; then
      echo "error: systemd --user is not available in the current session" >&2
      exit 1
    fi
    echo "  user service: skipped (systemd --user is not available in the current session)" >&2
    return 0
  fi

  unit_dir="$(systemd_user_dir)"
  unit_name="$(systemd_user_unit_name)"
  unit_path="${unit_dir}/${unit_name}"

  mkdir -p "${unit_dir}"
  write_systemd_user_service "${unit_path}" "${wrapper_path}"

  systemctl --user daemon-reload
  systemctl --user enable "${unit_name}" >/dev/null
  if systemctl --user is-active --quiet "${unit_name}"; then
    systemctl --user restart "${unit_name}" >/dev/null
  else
    systemctl --user start "${unit_name}" >/dev/null
  fi

  SERVICE_MANAGER="systemd-user"
  SERVICE_UNIT_PATH="${unit_path}"

  echo "  user service: ${unit_name}" >&2
  echo "  user service file: ${unit_path}" >&2

  linger_status="$(loginctl show-user "${USER}" -p Linger 2>/dev/null || true)"
  if [[ "${linger_status}" == "Linger=yes" ]]; then
    echo "  user service auto-start: enabled" >&2
  else
    echo "  user service auto-start: enabled for your user session/login" >&2
    echo "  note: to keep it running without an active login session, enable linger with: sudo loginctl enable-linger ${USER}" >&2
  fi
}

write_install_metadata() {
  local metadata_path="$1"
  local selected_target="$2"
  local wrapper_path="$3"
  local real_bin_path="$4"
  local ui_dist_path="$5"
  local runtime_args_path="$6"

  {
    printf 'CODEX_PROXY_METADATA_REPO=%q\n' "${REPO}"
    printf 'CODEX_PROXY_METADATA_VERSION=%q\n' "${VERSION}"
    printf 'CODEX_PROXY_METADATA_TARGET=%q\n' "${selected_target}"
    printf 'CODEX_PROXY_METADATA_INSTALL_BIN_DIR=%q\n' "${INSTALL_BIN_DIR}"
    printf 'CODEX_PROXY_METADATA_INSTALL_SHARE_DIR=%q\n' "${INSTALL_SHARE_DIR}"
    printf 'CODEX_PROXY_METADATA_WRAPPER_PATH=%q\n' "${wrapper_path}"
    printf 'CODEX_PROXY_METADATA_REAL_BIN_PATH=%q\n' "${real_bin_path}"
    printf 'CODEX_PROXY_METADATA_UI_DIST=%q\n' "${ui_dist_path}"
    printf 'CODEX_PROXY_METADATA_RUNTIME_ARGS_FILE=%q\n' "${runtime_args_path}"
    printf 'CODEX_PROXY_METADATA_SERVICE_MODE=%q\n' "${SERVICE_MODE}"
    printf 'CODEX_PROXY_METADATA_SERVICE_NAME=%q\n' "${SERVICE_NAME}"
    printf 'CODEX_PROXY_METADATA_SERVICE_MANAGER=%q\n' "${SERVICE_MANAGER}"
    printf 'CODEX_PROXY_METADATA_SERVICE_UNIT_PATH=%q\n' "${SERVICE_UNIT_PATH}"
  } > "${metadata_path}"

  chmod 600 "${metadata_path}"
}

print_runtime_args() {
  local -a rendered
  local index arg next

  if [[ "${#RUNTIME_ARGS[@]}" -eq 0 ]]; then
    echo "  saved runtime args: (none)"
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

  printf '  saved runtime args:'
  printf ' %q' "${rendered[@]}"
  printf '\n'
}

main() {
  local ext archive_name tmp_dir pkg_root selected_target download_result
  local real_bin_src real_bin_dst real_bin_tmp wrapper_dst ui_src ui_dst runtime_args_path metadata_path
  local -a target_candidates

  parse_args "$@"

  need_cmd curl
  need_cmd tar
  need_cmd mktemp

  ext="tar.gz"

  if [[ -n "${TARGET_OVERRIDE}" ]]; then
    target_candidates=("${TARGET_OVERRIDE}")
  else
    mapfile -t target_candidates < <(detect_target_candidates)
  fi

  if [[ "${#target_candidates[@]}" -eq 0 ]]; then
    echo "error: failed to determine release target candidates" >&2
    exit 1
  fi

  if [[ -z "${VERSION}" ]]; then
    VERSION="$(resolve_latest_version)"
    if [[ -z "${VERSION}" ]]; then
      echo "error: failed to resolve latest release version from ${REPO}" >&2
      exit 1
    fi
  fi

  tmp_dir="$(mktemp -d)"
  CLEANUP_TMP_DIR="${tmp_dir}"
  trap cleanup_tmp_dir EXIT

  if ! download_result="$(download_archive "${VERSION}" "${ext}" "${tmp_dir}" "${target_candidates[@]}")"; then
    echo "error: failed to download a release archive for ${VERSION}" >&2
    echo "tried targets: ${target_candidates[*]}" >&2
    exit 1
  fi

  IFS=$'\t' read -r selected_target archive_name <<< "${download_result}"

  echo "Extracting archive ..."
  tar -xzf "${tmp_dir}/${archive_name}" -C "${tmp_dir}"

  pkg_root="${tmp_dir}/codex-proxy-${selected_target}"
  if [[ ! -d "${pkg_root}" ]]; then
    echo "error: archive layout is not recognized (missing ${pkg_root})" >&2
    exit 1
  fi

  real_bin_src="${pkg_root}/codex-proxy"
  if [[ ! -f "${real_bin_src}" ]]; then
    echo "error: binary not found in archive: ${real_bin_src}" >&2
    exit 1
  fi

  mkdir -p "${INSTALL_BIN_DIR}"
  mkdir -p "${INSTALL_SHARE_DIR}/bin"
  mkdir -p "${INSTALL_SHARE_DIR}/ui"

  real_bin_dst="${INSTALL_SHARE_DIR}/bin/codex-proxy"
  wrapper_dst="${INSTALL_BIN_DIR}/codex-proxy"
  ui_src="${pkg_root}/ui/dist"
  ui_dst="${INSTALL_SHARE_DIR}/ui/dist"
  runtime_args_path="${INSTALL_SHARE_DIR}/runtime-args.sh"
  metadata_path="${INSTALL_SHARE_DIR}/install-metadata.env"

  stop_existing_user_service_if_running

  real_bin_tmp="${real_bin_dst}.tmp.$$"
  cp "${real_bin_src}" "${real_bin_tmp}"
  chmod +x "${real_bin_tmp}"
  mv -f "${real_bin_tmp}" "${real_bin_dst}"

  if [[ -d "${ui_src}" ]]; then
    rm -rf "${ui_dst}"
    cp -R "${ui_src}" "${ui_dst}"
  fi

  write_runtime_args "${runtime_args_path}" "${RUNTIME_ARGS[@]}"
  write_launcher_script "${wrapper_dst}" "${real_bin_dst}" "${runtime_args_path}" "${ui_dst}"
  install_user_service "${wrapper_dst}"
  write_install_metadata \
    "${metadata_path}" \
    "${selected_target}" \
    "${wrapper_dst}" \
    "${real_bin_dst}" \
    "${ui_dst}" \
    "${runtime_args_path}"

  echo
  echo "Installed codex-proxy ${VERSION} (${selected_target})"
  echo "  launcher: ${wrapper_dst}"
  echo "  real binary: ${real_bin_dst}"
  if [[ -d "${ui_dst}" ]]; then
    echo "  ui dist: ${ui_dst}"
  fi
  echo "  runtime args file: ${runtime_args_path}"
  echo "  metadata: ${metadata_path}"
  print_runtime_args
  echo
  echo "Installed launcher runs as the current user. No system-level service was created."
  if [[ "${SERVICE_MANAGER}" == "systemd-user" ]]; then
    echo "User service is installed and running: $(systemd_user_unit_name)"
    echo "Manage it with:"
    echo "  systemctl --user status $(systemd_user_unit_name)"
    echo "  systemctl --user restart $(systemd_user_unit_name)"
    echo "  systemctl --user stop $(systemd_user_unit_name)"
    echo
  fi
  echo
  if [[ ":${PATH}:" != *":${INSTALL_BIN_DIR}:"* ]]; then
    echo "Add ${INSTALL_BIN_DIR} to PATH:"
    echo "  export PATH=\"${INSTALL_BIN_DIR}:\$PATH\""
    echo
  fi
  if [[ "${#RUNTIME_ARGS[@]}" -eq 0 ]]; then
    echo "No default runtime args were saved."
    echo "Run manually when needed, for example:"
    echo "  ${wrapper_dst} --bind 127.0.0.1:8787 --data-dir ${INSTALL_SHARE_DIR}/data --admin-password change-me"
  elif [[ "${SERVICE_MANAGER}" != "systemd-user" ]]; then
    echo "Start the proxy with the saved args:"
    echo "  ${wrapper_dst}"
    echo
    echo "Additional CLI args are appended after the saved args."
    echo "If you need to change a saved option such as --bind or --data-dir, rerun update.sh with a new runtime-args section."
  else
    echo "The proxy has already been started through the user service."
    echo "If you need to change a saved option such as --bind or --data-dir, rerun update.sh with a new runtime-args section."
  fi
}

main "$@"
