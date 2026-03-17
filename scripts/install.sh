#!/usr/bin/env bash

set -euo pipefail

REPO_OWNER="${HITCH_REPO_OWNER:-EngoDev}"
REPO_NAME="${HITCH_REPO_NAME:-hitch}"
INSTALL_ROOT_SYSTEM="/usr/local/bin"
INSTALL_ROOT_USER="${HOME}/.local/bin"

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command '$1' is not installed" >&2
    exit 1
  fi
}

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  if [ "$os" != "Linux" ]; then
    echo "error: hitch installer currently supports Linux only" >&2
    exit 1
  fi

  case "$arch" in
    x86_64)
      printf '%s\n' "x86_64-unknown-linux-gnu"
      ;;
    aarch64|arm64)
      printf '%s\n' "aarch64-unknown-linux-gnu"
      ;;
    *)
      echo "error: unsupported Linux architecture: $arch" >&2
      exit 1
      ;;
  esac
}

resolve_version() {
  if [ -n "${HITCH_VERSION:-}" ]; then
    printf '%s\n' "$HITCH_VERSION"
    return
  fi

  curl -fsSL "https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}/releases/latest" \
    | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' \
    | head -n 1
}

select_install_dir() {
  if [ -d "$INSTALL_ROOT_SYSTEM" ] && [ -w "$INSTALL_ROOT_SYSTEM" ]; then
    printf '%s\n' "$INSTALL_ROOT_SYSTEM"
    return
  fi

  mkdir -p "$INSTALL_ROOT_USER"
  if [ -w "$INSTALL_ROOT_USER" ]; then
    printf '%s\n' "$INSTALL_ROOT_USER"
    return
  fi

  echo "error: no writable install directory found" >&2
  exit 1
}

main() {
  require_command curl
  require_command mktemp
  require_command chmod
  require_command mv

  local target version asset_name download_url install_dir tmpfile
  target="$(detect_target)"
  version="$(resolve_version)"

  if [ -z "$version" ]; then
    echo "error: could not resolve hitch release version" >&2
    exit 1
  fi

  asset_name="hitch-${version}-${target}"
  download_url="https://github.com/${REPO_OWNER}/${REPO_NAME}/releases/download/${version}/${asset_name}"
  install_dir="$(select_install_dir)"
  tmpfile="$(mktemp)"

  trap 'rm -f "$tmpfile"' EXIT

  echo "Installing hitch ${version} for ${target}..."
  curl -fL "$download_url" -o "$tmpfile"

  chmod +x "$tmpfile"
  mv "$tmpfile" "${install_dir}/hitch"
  trap - EXIT

  echo "Installed hitch to ${install_dir}/hitch"

  case ":${PATH}:" in
    *":${install_dir}:"*) ;;
    *)
      echo "warning: ${install_dir} is not on your PATH" >&2
      ;;
  esac
}

main "$@"
