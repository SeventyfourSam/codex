#!/bin/sh
# GitHub-only installer for macOS arm64. Keep the upstream installer independent.
set -eu
umask 077
repo=https://github.com/SeventyfourSam/codex
release=${CODEX_RELEASE:-latest}
if [ "${1:-}" = --release ] && [ "$#" = 2 ]; then release=$2
elif [ "$#" != 0 ]; then echo 'Usage: install.sh [--release VERSION]' >&2; exit 1; fi
[ "$(uname -s)" = Darwin ] || { echo 'Only macOS arm64 is supported.' >&2; exit 1; }
arch=$(uname -m)
if [ "$arch" = x86_64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || true)" = 1 ]; then arch=arm64; fi
[ "$arch" = arm64 ] || { echo 'Only macOS arm64 is supported.' >&2; exit 1; }
for command in curl tar shasum; do command -v "$command" >/dev/null; done
fetch() { curl -fsSL --connect-timeout 10 --max-time 300 "$1" -o "$2"; }
if [ "$release" = latest ]; then
  url=$(curl -fsSL --connect-timeout 10 --max-time 30 -I -o /dev/null -w '%{url_effective}' "$repo/releases/latest")
  case "$url" in "$repo/releases/tag/v"*) version=${url#"$repo/releases/tag/v"};; *) echo 'Invalid latest release redirect.' >&2; exit 1;; esac
else version=${release#v}; fi
printf '%s\n' "$version" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-custom(\.(0|[1-9][0-9]*))?$' || { echo 'Expected x.y.z-custom or x.y.z-custom.N.' >&2; exit 1; }
base=${version%-custom*}
target=aarch64-apple-darwin
name=$version-$target
codex_home=${CODEX_HOME:-$HOME/.codex}
root=$codex_home/packages/standalone
if [ "${CODEX_INSTALL_DAEMON_ONLY:-0}" = 1 ]; then root=$codex_home/packages/app-server-daemon; fi
mkdir -p "$root/releases"
root=$(cd "$root" && pwd -P)
# The reported macOS endpoint policy permits the exact /release/ path component.
# Use it for both staging and installed CLI packages; preserve the daemon layout.
releases=$root/releases
if [ "${CODEX_INSTALL_DAEMON_ONLY:-0}" != 1 ]; then releases=$releases/release; fi
mkdir -p "$releases"
current=$root/current
if [ "${CODEX_INSTALL_DEFER_SELECTION:-0}" = 1 ]; then
  [ "${CODEX_INSTALL_DAEMON_ONLY:-0}" = 1 ] && [ ! -e "$current" ] || exit 1
  current=$root/.migration-current
fi
# Match the daemon's local installer lock protocol.
lock=$root/install.lock.d
owned_lock=0
if command -v lockf >/dev/null 2>&1; then
  : >> "$root/install.lock"; exec 9<>"$root/install.lock"; lockf 9
elif command -v flock >/dev/null 2>&1; then
  exec 9>"$root/install.lock"; flock 9
else
  attempt=0
  until mkdir "$lock" 2>/dev/null; do
    attempt=$((attempt + 1))
    [ "$attempt" -lt 60 ] || { echo "Installation busy: $lock" >&2; exit 1; }
    sleep 1
  done
  owned_lock=1
  printf '%s' "$$" > "$lock/pid"; date +%s > "$lock/started_at"
fi
stage=
cleanup() {
  [ -z "$stage" ] || rm -rf "$stage"
  rm -f "$root/.custom-current.$$"
  if [ "$owned_lock" = 1 ]; then rm -f "$lock/pid" "$lock/started_at"; rmdir "$lock"; fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
if [ "${CODEX_INSTALL_DEFER_SELECTION:-0}" = 1 ] && [ -e "$root/current" ]; then exit 1; fi
# Preserve pins and recheck the selection before a daemon-owned update.
if [ "${CODEX_INSTALL_IF_LATEST:-0}" = 1 ] || [ "${CODEX_INSTALL_IF_CURRENT:-0}" = 1 ]; then
  selected=$(cd "$current" 2>/dev/null && pwd -P) || exit 1
  [ -n "${CODEX_UPDATE_FROM_RELEASE:-}" ] && [ "$selected" = "$releases/$CODEX_UPDATE_FROM_RELEASE" ] || exit 1
  if [ "${CODEX_INSTALL_IF_LATEST:-0}" = 1 ]; then
    [ "$(cat "$root/auto-update-version" 2>/dev/null || true)" = "$CODEX_UPDATE_FROM_RELEASE" ] || exit 0
  fi
fi
package_complete() {
  [ -f "$1/codex-package.json" ] && [ -x "$1/bin/codex" ] && [ -x "$1/bin/codex-code-mode-host" ] && [ -x "$1/codex-path/rg" ] && [ -x "$1/codex-resources/zsh/bin/zsh" ] &&
    [ "$("$1/bin/codex" --version)" = "codex-cli $base" ] && [ "$("$1/bin/codex" --custom)" = "codex-cli $version" ]
}
destination=$releases/$name
if ! package_complete "$destination"; then
  [ ! -e "$destination" ] || { echo "Incomplete release exists: $destination; remove it manually before retrying." >&2; exit 1; }
  stage=$(mktemp -d "$releases/.custom-stage.XXXXXX")
  asset=codex-$name.tar.gz
  fetch "$repo/releases/download/v$version/SHA256SUMS" "$stage/SHA256SUMS"
  digest=$(awk -v asset="$asset" '$2 == asset && length($1) == 64 && $1 !~ /[^0-9a-fA-F]/ { digest=$1; count++ } END { if (count != 1) exit 1; print digest }' "$stage/SHA256SUMS")
  fetch "$repo/releases/download/v$version/$asset" "$stage/archive.tar.gz"
  actual=$(shasum -a 256 "$stage/archive.tar.gz" | awk '{print $1}')
  [ "$actual" = "$digest" ] || { echo 'Package SHA-256 mismatch.' >&2; exit 1; }
  mkdir "$stage/package"
  tar -xzf "$stage/archive.tar.gz" -C "$stage/package"
  package_complete "$stage/package" || { echo 'Package validation failed.' >&2; exit 1; }
  if [ "${CODEX_INSTALL_DAEMON_ONLY:-0}" = 1 ]; then "$stage/package/bin/codex" app-server daemon pid-update-loop --check-package-ownership; fi
  mv "$stage/package" "$destination"
fi
CODEX_HOME="$codex_home" "$destination/bin/codex" --initialize-custom-config
[ ! -e "$current" ] || [ -L "$current" ] || { echo 'Refusing to replace an unmanaged current directory.' >&2; exit 1; }
ln -s "$destination" "$root/.custom-current.$$"
mv -fh "$root/.custom-current.$$" "$current"
if [ "$release" = latest ]; then printf '%s' "$name" > "$root/auto-update-version.tmp.$$"; mv -f "$root/auto-update-version.tmp.$$" "$root/auto-update-version"
else rm -f "$root/auto-update-version"; fi
if [ "${CODEX_INSTALL_DAEMON_ONLY:-0}" = 1 ]; then exit 0; fi
bin=${CODEX_INSTALL_DIR:-$HOME/.local/bin}
mkdir -p "$bin"
bin=$(cd "$bin" && pwd -P)
for executable in codex codex-code-mode-host; do
  [ ! -d "$bin/$executable" ] || [ -L "$bin/$executable" ] || { echo "Refusing to replace directory: $bin/$executable" >&2; exit 1; }
  ln -s "$current/bin/$executable" "$bin/.$executable.$$"
  mv -fh "$bin/.$executable.$$" "$bin/$executable"
done
[ "$("$bin/codex" --custom)" = "codex-cli $version" ] || exit 1
# Keep a single, sourceable PATH entry, including paths containing spaces.
# shellcheck disable=SC2016
printf 'export PATH="%s:$PATH"\n' "$(printf '%s' "$bin" | sed 's/[\\"$`]/\\&/g')" > "$root/env"
case "${SHELL:-}" in */zsh) profile=${ZDOTDIR:-$HOME}/.zshrc;; *) profile=$HOME/.profile;; esac
entry=". \"$(printf '%s' "$root/env" | sed 's/[\\"$`]/\\&/g')\""
[ -f "$profile" ] && grep -Fxq "$entry" "$profile" || printf '\n%s\n' "$entry" >> "$profile"
printf 'Installed %s. Start a new terminal, or run: "%s/codex"\n' "$version" "$bin"
