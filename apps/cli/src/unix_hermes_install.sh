#!/usr/bin/env bash
set -euo pipefail

case "$(uname -s)" in Darwin|Linux) ;; *) echo 'Hermes setup supports macOS and Linux.' >&2; exit 1;; esac
[ "$(id -u)" != 0 ] || { echo 'Run Hermes setup as your normal user, without sudo.' >&2; exit 1; }
command -v git >/dev/null || { echo 'Install Git, then run setup again.' >&2; exit 1; }
python=''
for candidate in python3.13 python3.12 python3.11 python3; do
  if command -v "$candidate" >/dev/null && "$candidate" -c 'import sys; sys.exit(not ((3,11) <= sys.version_info[:2] < (3,14)))' 2>/dev/null; then
    python="$candidate"; break
  fi
done
[ -n "$python" ] || { echo 'Install Python 3.11, 3.12 or 3.13 with venv support, then run setup again.' >&2; exit 1; }

root="${HERMES_HOME:-$HOME/.hermes}"
project="$root/hermes-agent"
commit=9de9c25f620ff7f1ce0fd5457d596052d5159596
mkdir -p "$root"
lock="$root/.mundusx-install.lock"
mkdir "$lock" 2>/dev/null || { echo 'Another Hermes installation may be running. Finish it before retrying.' >&2; exit 1; }
trap 'rmdir "$lock"' EXIT
if [ -e "$project" ] && [ ! -d "$project/.git" ]; then
  echo "Existing Hermes directory is not a managed Git checkout: $project. Nothing was replaced." >&2; exit 1
fi
if [ ! -d "$project/.git" ]; then
  mkdir -p "$project"
  git -C "$project" init --quiet
fi
if git -C "$project" rev-parse --verify HEAD >/dev/null 2>&1; then
  [ "$(git -C "$project" rev-parse HEAD)" = "$commit" ] || {
    echo 'An existing Hermes checkout uses another revision. It was preserved; update it separately.' >&2; exit 1;
  }
  [ -z "$(git -C "$project" status --porcelain --untracked-files=no)" ] || {
    echo 'The Hermes checkout has local edits. It was preserved.' >&2; exit 1;
  }
else
  git -C "$project" fetch --depth 1 https://github.com/NousResearch/hermes-agent.git "$commit"
  git -C "$project" checkout --detach "$commit"
fi
[ "$(git -C "$project" rev-parse HEAD)" = "$commit" ] || { echo 'Hermes revision verification failed.' >&2; exit 1; }
if [ ! -x "$project/venv/bin/python" ]; then "$python" -m venv "$project/venv"; fi
"$project/venv/bin/python" -m pip install --disable-pip-version-check -e "$project"
"$project/venv/bin/hermes" --version
mkdir -p "$root/bin"
if [ -e "$root/bin/hermes" ] && [ ! -L "$root/bin/hermes" ]; then
  echo 'An existing Hermes launcher was preserved; refusing to replace it.' >&2; exit 1
fi
ln -sfn "$project/venv/bin/hermes" "$root/bin/hermes"
echo 'Hermes installed. Existing configuration, conversations and credentials were preserved.'
