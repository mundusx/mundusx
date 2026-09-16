#!/usr/bin/env bash
set -euo pipefail
[ "$(id -u)" != 0 ] || { echo 'Run as your normal user, without sudo.' >&2; exit 1; }
bundle="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
case "$(uname -s)/$(uname -m)" in
  Darwin/arm64) target=aarch64-apple-darwin;;
  Darwin/x86_64) target=x86_64-apple-darwin;;
  Linux/aarch64|Linux/arm64) target=aarch64-unknown-linux-gnu;;
  Linux/x86_64) target=x86_64-unknown-linux-gnu;;
  *) echo 'Unsupported operating system or architecture.' >&2; exit 1;;
esac
[ "$(cat "$bundle/TARGET")" = "$target" ] || { echo 'This installer does not match this computer.' >&2; exit 1; }
cd "$bundle"
if command -v sha256sum >/dev/null; then sha256sum -c SHA256SUMS; else shasum -a 256 -c SHA256SUMS; fi
bin="${INSTALL_DIR:-$HOME/.local/bin}"
mkdir -p "$bin"
stamp="$(date +%Y%m%d-%H%M%S)"
for name in mundusx mundusx-agent-server; do
  [ ! -e "$bin/$name" ] || cp -p "$bin/$name" "$bin/$name.backup-$stamp"
  install -m 755 "$bundle/$name" "$bin/$name.new"
  "$bin/$name.new" --version
  mv -f "$bin/$name.new" "$bin/$name"
done
export PATH="$bin:$PATH"
"$bin/mundusx" agent install hermes
INSTALL_DIR="$bin" bash "$bundle/install-chat-service.sh" --configure-chat-service
printf '\nInstalled MundusX + Hermes. Run "%s/mundusx connect" to approve this computer in Chat.\n' "$bin"
