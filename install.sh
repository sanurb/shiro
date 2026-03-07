#!/usr/bin/env bash

set -euo pipefail

REPO_OWNER="sanurb"
REPO_NAME="shiro"
REPO="${REPO_OWNER}/${REPO_NAME}"

info() {
  printf '%s\n' "shiro-install: $*" >&2
}

error() {
  printf '%s\n' "shiro-install: ERROR: $*" >&2
  exit 1
}

have_cmd() {
  command -v "$1" >/dev/null 2>&1
}

detect_target() {
  local os arch
  os="$(uname -s 2>/dev/null || echo unknown)"
  arch="$(uname -m 2>/dev/null || echo unknown)"

  case "$os" in
    Linux)
      case "$arch" in
        x86_64) echo "x86_64-unknown-linux-gnu" ;;
        aarch64 | arm64) echo "aarch64-unknown-linux-gnu" ;;
        *)
          info "Unsupported Linux architecture: ${arch}"
          return 1
          ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        x86_64) echo "x86_64-apple-darwin" ;;
        arm64) echo "aarch64-apple-darwin" ;;
        *)
          info "Unsupported macOS architecture: ${arch}"
          return 1
          ;;
      esac
      ;;
    *)
      info "Unsupported OS: ${os}"
      return 1
      ;;
  esac
}

fetch_latest_tag() {
  if ! have_cmd curl; then
    error "curl is required to fetch the latest release."
  fi

  local api_url="https://api.github.com/repos/${REPO}/releases/latest"
  local json
  json="$(curl -sSfL "${api_url}")" || error "Failed to query GitHub releases API."

  local tag
  tag="$(printf '%s\n' "${json}" | sed -n 's/  *\"tag_name\": *\"\\(.*\\)\",/\\1/p' | head -n 1)"

  if [ -z "${tag:-}" ]; then
    error "Unable to determine latest tag from GitHub API response."
  fi

  printf '%s\n' "${tag}"
}

install_from_release() {
  local target
  if ! target="$(detect_target)"; then
    return 1
  fi

  local tag
  tag="$(fetch_latest_tag)"

  local archive="shiro-${tag}-${target}"
  local tarball="${archive}.tar.gz"
  local url="https://github.com/${REPO}/releases/download/${tag}/${tarball}"

  info "Detected platform: ${target}"
  info "Latest release: ${tag}"
  info "Downloading ${tarball} from GitHub releases..."

  local tmpdir
  if tmpdir="$(mktemp -d 2>/dev/null)"; then
    :
  else
    tmpdir="$(mktemp -d -t shiro-install)"
  fi

  trap 'rm -rf "${tmpdir}"' EXIT

  if ! curl -sSfL "${url}" -o "${tmpdir}/${tarball}"; then
    info "Download of ${tarball} failed."
    return 1
  fi

  (
    cd "${tmpdir}"
    tar xzf "${tarball}"
  ) || error "Failed to extract release archive."

  local install_dir="${SHIRO_INSTALL_DIR:-${HOME}/.local/bin}"
  mkdir -p "${install_dir}"

  cp "${tmpdir}/${archive}/shiro" "${install_dir}/shiro"
  chmod +x "${install_dir}/shiro"

  info "Installed shiro to ${install_dir}/shiro"
  info "Ensure ${install_dir} is on your PATH."
}

install_via_cargo() {
  if ! have_cmd cargo; then
    error "cargo not found. Install Rust from https://rustup.rs or use a prebuilt binary."
  fi

  info "Installing via cargo (crate: shiro-cli)..."
  cargo install shiro-cli
}

main() {
  if [ "${SHIRO_USE_CARGO:-0}" = "1" ]; then
    install_via_cargo
    exit 0
  fi

  if ! install_from_release; then
    info "Falling back to cargo install..."
    install_via_cargo
  fi

  info "Installation complete. Run 'shiro --help' to get started."
}

main "$@"

