#!/usr/bin/env bash
set -euo pipefail

REPO=""
TAG=""
SCRIPT_REF=""
TMP_ROOT=""
DBUS_DAEMON_PID=""
SYSTEMD_USER_PID=""
SERVICE_NAME=""

usage() {
  cat <<'EOF'
Usage:
  test-release-install-flow.sh --repo <owner/name> --tag <tag> [--script-ref <ref>]

Options:
  --repo <owner/name>   GitHub repository that owns the release
  --tag <tag>           Release tag to validate, for example v0.1.3
  --script-ref <ref>    Git ref used to fetch install/update/uninstall scripts
                        Defaults to the same value as --tag
  -h, --help            Show this help
EOF
}

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command not found: $1" >&2
    exit 1
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
        shift 2
        ;;
      --repo=*)
        REPO="${1#*=}"
        shift
        ;;
      --tag)
        require_option_value "$1" "${2:-}"
        TAG="$2"
        shift 2
        ;;
      --tag=*)
        TAG="${1#*=}"
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
      *)
        echo "error: unknown option: $1" >&2
        usage >&2
        exit 1
        ;;
    esac
  done

  if [[ -z "${REPO}" || -z "${TAG}" ]]; then
    usage >&2
    exit 1
  fi

  if [[ -z "${SCRIPT_REF}" ]]; then
    SCRIPT_REF="${TAG}"
  fi
}

cleanup() {
  if [[ -n "${SYSTEMD_USER_PID}" ]]; then
    kill "${SYSTEMD_USER_PID}" >/dev/null 2>&1 || true
    wait "${SYSTEMD_USER_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${DBUS_DAEMON_PID}" ]]; then
    kill "${DBUS_DAEMON_PID}" >/dev/null 2>&1 || true
    wait "${DBUS_DAEMON_PID}" >/dev/null 2>&1 || true
  fi
  if [[ -n "${TMP_ROOT}" ]]; then
    rm -rf "${TMP_ROOT}"
  fi
}

raw_script_url() {
  local script_name="$1"
  printf 'https://raw.githubusercontent.com/%s/%s/scripts/%s\n' "${REPO}" "${SCRIPT_REF}" "${script_name}"
}

wait_for_user_manager() {
  local attempt
  for attempt in $(seq 1 120); do
    if [[ -n "${SYSTEMD_USER_PID}" ]] && ! kill -0 "${SYSTEMD_USER_PID}" >/dev/null 2>&1; then
      echo "error: systemd --user exited before becoming ready" >&2
      if [[ -n "${TMP_ROOT}" && -f "${TMP_ROOT}/systemd-user.log" ]]; then
        sed -n '1,200p' "${TMP_ROOT}/systemd-user.log" >&2
      fi
      return 1
    fi

    if systemctl --user is-active default.target >/dev/null 2>&1; then
      return 0
    fi

    # Some CI runners accept user-manager commands before default.target flips to
    # "active". That is sufficient for the install/update/uninstall flow below.
    if systemctl --user show-environment >/dev/null 2>&1 \
      && [[ "$(systemctl --user show default.target --property=LoadState --value 2>/dev/null)" == "loaded" ]]; then
      return 0
    fi

    sleep 0.5
  done

  echo "error: timed out waiting for systemd --user readiness" >&2
  if [[ -n "${TMP_ROOT}" && -f "${TMP_ROOT}/systemd-user.log" ]]; then
    sed -n '1,200p' "${TMP_ROOT}/systemd-user.log" >&2
  fi
  systemctl --user --no-pager status default.target >&2 || true
  return 1
}

wait_for_http() {
  local url="$1"
  local attempt
  for attempt in $(seq 1 60); do
    if curl -fsS "${url}" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.5
  done

  echo "error: timed out waiting for ${url}" >&2
  return 1
}

ensure_not_enabled() {
  local unit_name="$1"
  if systemctl --user is-enabled "${unit_name}" >/dev/null 2>&1; then
    echo "error: expected ${unit_name} to be disabled" >&2
    exit 1
  fi
}

ensure_not_active() {
  local unit_name="$1"
  if systemctl --user is-active --quiet "${unit_name}"; then
    echo "error: expected ${unit_name} to be inactive" >&2
    exit 1
  fi
}

start_user_service_manager() {
  local runtime_dir config_home

  TMP_ROOT="$(mktemp -d)"
  runtime_dir="${TMP_ROOT}/runtime"
  config_home="${TMP_ROOT}/config"

  mkdir -p "${runtime_dir}" "${config_home}"
  chmod 700 "${runtime_dir}"

  export XDG_RUNTIME_DIR="${runtime_dir}"
  export XDG_CONFIG_HOME="${config_home}"
  export DBUS_SESSION_BUS_ADDRESS="unix:path=${XDG_RUNTIME_DIR}/bus"

  DBUS_DAEMON_PID="$(
    dbus-daemon \
      --session \
      --address="${DBUS_SESSION_BUS_ADDRESS}" \
      --fork \
      --nopidfile \
      --print-pid=1
  )"

  SYSTEMD_LOG_LEVEL=err systemd --user > "${TMP_ROOT}/systemd-user.log" 2>&1 &
  SYSTEMD_USER_PID="$!"

  wait_for_user_manager
}

main() {
  local install_root bin_dir share_dir data_dir port install_log
  local gnu_root gnu_bin_dir gnu_share_dir gnu_data_dir gnu_log
  local update_log uninstall_log unit_name unit_path wants_link
  local service_pid_before service_pid_after

  parse_args "$@"

  need_cmd curl
  need_cmd grep
  need_cmd mktemp
  need_cmd dbus-daemon
  need_cmd systemd
  need_cmd systemctl

  trap cleanup EXIT
  start_user_service_manager

  SERVICE_NAME="codex-proxy-ci-$$"
  unit_name="${SERVICE_NAME}.service"
  unit_path="${XDG_CONFIG_HOME}/systemd/user/${unit_name}"
  wants_link="${XDG_CONFIG_HOME}/systemd/user/default.target.wants/${unit_name}"

  install_root="${TMP_ROOT}/install-main"
  bin_dir="${install_root}/bin"
  share_dir="${install_root}/share"
  data_dir="${install_root}/data"
  port=18787
  install_log="${install_root}/install.log"
  mkdir -p "${bin_dir}" "${share_dir}"

  curl -fsSL "$(raw_script_url install.sh)" \
    | bash -s -- \
        --repo "${REPO}" \
        --version "${TAG}" \
        --install-bin-dir "${bin_dir}" \
        --install-share-dir "${share_dir}/codex-proxy" \
        --service-name "${SERVICE_NAME}" \
        -- \
        --bind "127.0.0.1:${port}" \
        --data-dir "${data_dir}" \
        --admin-password "test-admin-password" | tee "${install_log}"

  grep -q "x86_64-unknown-linux-musl" "${install_log}"
  grep -q "User service is installed and running:" "${install_log}"
  systemctl --user is-enabled "${unit_name}" >/dev/null
  systemctl --user is-active "${unit_name}" >/dev/null
  test -f "${unit_path}"
  test -L "${wants_link}"
  test -x "${bin_dir}/codex-proxy"
  test -x "${share_dir}/codex-proxy/bin/codex-proxy"
  test -f "${share_dir}/codex-proxy/ui/dist/index.html"
  test -f "${share_dir}/codex-proxy/install-metadata.env"
  test -f "${share_dir}/codex-proxy/runtime-args.sh"

  service_pid_before="$(systemctl --user show "${unit_name}" --property MainPID --value)"
  if [[ -z "${service_pid_before}" || "${service_pid_before}" == "0" ]]; then
    echo "error: failed to capture pre-update MainPID for ${unit_name}" >&2
    exit 1
  fi

  wait_for_http "http://127.0.0.1:${port}/healthz"
  curl -fsS "http://127.0.0.1:${port}/readyz" >/dev/null
  curl -fsS "http://127.0.0.1:${port}/" | grep -qi "<html"

  tag_url="https://github.com/${REPO}/releases/download/${TAG}/codex-proxy-${TAG}-x86_64-unknown-linux-musl.tar.gz"
  curl -fsI "${tag_url}" >/dev/null

  gnu_root="${TMP_ROOT}/install-gnu"
  gnu_bin_dir="${gnu_root}/bin"
  gnu_share_dir="${gnu_root}/share"
  gnu_data_dir="${gnu_root}/data"
  gnu_log="${gnu_root}/install-gnu.log"
  mkdir -p "${gnu_bin_dir}" "${gnu_share_dir}"

  curl -fsSL "$(raw_script_url install.sh)" \
    | bash -s -- \
        --repo "${REPO}" \
        --version "${TAG}" \
        --target "x86_64-unknown-linux-gnu" \
        --install-bin-dir "${gnu_bin_dir}" \
        --install-share-dir "${gnu_share_dir}/codex-proxy" \
        --service-mode none \
        --service-name "${SERVICE_NAME}-gnu" \
        -- \
        --bind "127.0.0.1:18788" \
        --data-dir "${gnu_data_dir}" \
        --admin-password "test-admin-password" | tee "${gnu_log}"

  grep -q "x86_64-unknown-linux-gnu" "${gnu_log}"
  test -x "${gnu_bin_dir}/codex-proxy"
  test -x "${gnu_share_dir}/codex-proxy/bin/codex-proxy"
  test -f "${gnu_share_dir}/codex-proxy/ui/dist/index.html"
  test ! -e "${XDG_CONFIG_HOME}/systemd/user/${SERVICE_NAME}-gnu.service"

  update_log="${install_root}/update.log"
  curl -fsSL "$(raw_script_url update.sh)" \
    | bash -s -- \
        --repo "${REPO}" \
        --version "${TAG}" \
        --script-ref "${SCRIPT_REF}" \
        --install-bin-dir "${bin_dir}" \
        --install-share-dir "${share_dir}/codex-proxy" | tee "${update_log}"

  grep -q "Updating codex-proxy using" "${update_log}"
  grep -q "Reusing runtime args:" "${update_log}"
  systemctl --user is-enabled "${unit_name}" >/dev/null
  systemctl --user is-active "${unit_name}" >/dev/null

  wait_for_http "http://127.0.0.1:${port}/healthz"
  curl -fsS "http://127.0.0.1:${port}/readyz" >/dev/null

  service_pid_after="$(systemctl --user show "${unit_name}" --property MainPID --value)"
  if [[ -z "${service_pid_after}" || "${service_pid_after}" == "0" ]]; then
    echo "error: failed to capture post-update MainPID for ${unit_name}" >&2
    exit 1
  fi
  if [[ "${service_pid_before}" == "${service_pid_after}" ]]; then
    echo "error: expected ${unit_name} to restart during update" >&2
    exit 1
  fi

  uninstall_log="${install_root}/uninstall.log"
  curl -fsSL "$(raw_script_url uninstall.sh)" \
    | bash -s -- \
        --install-bin-dir "${bin_dir}" \
        --install-share-dir "${share_dir}/codex-proxy" \
        --remove-data-dir | tee "${uninstall_log}"

  grep -q "Removed user service: ${unit_name}" "${uninstall_log}"
  grep -q "Uninstalled codex-proxy." "${uninstall_log}"
  ensure_not_enabled "${unit_name}"
  ensure_not_active "${unit_name}"
  test ! -e "${unit_path}"
  test ! -e "${wants_link}"
  test ! -e "${bin_dir}/codex-proxy"
  test ! -d "${share_dir}/codex-proxy"
  test ! -e "${data_dir}"
}

main "$@"
