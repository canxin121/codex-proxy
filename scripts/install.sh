#!/usr/bin/env bash
set -euo pipefail

REPO="${CODEX_PROXY_REPO:-canxin121/codex-proxy}"
VERSION="${CODEX_PROXY_VERSION:-}"
TARGET_OVERRIDE="${CODEX_PROXY_TARGET:-}"
INSTALL_BIN_DIR="${CODEX_PROXY_INSTALL_BIN_DIR:-$HOME/.local/bin}"
INSTALL_SHARE_DIR="${CODEX_PROXY_INSTALL_SHARE_DIR:-$HOME/.local/share/codex-proxy}"

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command not found: $1" >&2
    exit 1
  fi
}

detect_target_candidates() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "${os}" in
    Linux)
      case "${arch}" in
        x86_64|amd64)
          # Prefer musl for better cross-distro runtime compatibility.
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
        x86_64|amd64) echo "x86_64-apple-darwin" ;;
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
    echo "Trying ${archive_name} ..."
    if curl -fsSL "${download_url}" -o "${tmp_dir}/${archive_name}"; then
      printf '%s\t%s\n' "${target}" "${archive_name}"
      return 0
    fi
    rm -f "${tmp_dir}/${archive_name}"
  done

  return 1
}

main() {
  need_cmd curl
  need_cmd tar
  need_cmd mktemp

  local ext archive_name tmp_dir pkg_root bin_src bin_dst ui_src ui_dst selected_target download_result
  local -a target_candidates

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
  trap 'rm -rf "${tmp_dir}"' EXIT

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

  bin_src="${pkg_root}/codex-proxy"
  if [[ ! -f "${bin_src}" ]]; then
    echo "error: binary not found in archive: ${bin_src}" >&2
    exit 1
  fi

  mkdir -p "${INSTALL_BIN_DIR}"
  mkdir -p "${INSTALL_SHARE_DIR}/ui"

  bin_dst="${INSTALL_BIN_DIR}/codex-proxy"
  cp "${bin_src}" "${bin_dst}"
  chmod +x "${bin_dst}"

  ui_src="${pkg_root}/ui/dist"
  ui_dst="${INSTALL_SHARE_DIR}/ui/dist"
  if [[ -d "${ui_src}" ]]; then
    rm -rf "${ui_dst}"
    cp -R "${ui_src}" "${ui_dst}"
  fi

  echo
  echo "Installed codex-proxy ${VERSION} (${selected_target})"
  echo "  binary: ${bin_dst}"
  if [[ -d "${ui_dst}" ]]; then
    echo "  ui dist: ${ui_dst}"
  fi
  echo
  if [[ ":${PATH}:" != *":${INSTALL_BIN_DIR}:"* ]]; then
    echo "Add ${INSTALL_BIN_DIR} to PATH:"
    echo "  export PATH=\"${INSTALL_BIN_DIR}:\$PATH\""
    echo
  fi
  echo "Optional: force UI path explicitly:"
  echo "  export CODEX_PROXY_UI_DIST_DIR=\"${ui_dst}\""
}

main "$@"
