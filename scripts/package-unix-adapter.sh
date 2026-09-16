#!/usr/bin/env bash
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
target="${1:?Usage: package-unix-adapter.sh TARGET BINARY_DIR OUTPUT_DIR}"
binary_dir="${2:?Missing binary directory}"
output="${3:?Missing output directory}"
case "$target" in
  aarch64-apple-darwin) label=macos-arm64;;
  x86_64-apple-darwin) label=macos-x64;;
  aarch64-unknown-linux-gnu) label=linux-arm64;;
  x86_64-unknown-linux-gnu) label=linux-x64;;
  *) echo 'Unsupported target' >&2; exit 1;;
esac
mkdir -p "$output"
output="$(cd "$output" && pwd -P)"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
for name in mundusx mundusx-agent-server; do
  test -f "$binary_dir/$name"
  cp "$binary_dir/$name" "$stage/$name"
  chmod 755 "$stage/$name"
done
cp "$repo/scripts/install-unix-adapter.sh" "$stage/setup.sh"
cp "$repo/install.sh" "$stage/install-chat-service.sh"
printf '%s\n' "$target" > "$stage/TARGET"
chmod 755 "$stage/setup.sh" "$stage/install-chat-service.sh"
hash() { if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi; }
(cd "$stage"; hash mundusx mundusx-agent-server setup.sh install-chat-service.sh TARGET > SHA256SUMS)
name="mundusx-hermes-$label.tar.gz"
tar -czf "$output/$name" -C "$stage" .
(cd "$output"; hash "$name" > "$name.sha256")
echo "Created $output/$name. Test setup on $label before publishing."
