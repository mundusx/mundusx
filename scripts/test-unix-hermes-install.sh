#!/usr/bin/env bash
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/bin" "$fixture/home/.hermes"
export HOME="$fixture/home" HERMES_HOME="$fixture/home/.hermes"
export PATH="$fixture/bin:$PATH"
printf 'saved configuration\n' > "$HERMES_HOME/config.yaml"
printf '#!/usr/bin/env bash\necho "${TEST_OS:-Linux}"\n' > "$fixture/bin/uname"
printf '#!/usr/bin/env bash\necho 1000\n' > "$fixture/bin/id"
cat > "$fixture/bin/git" <<'GIT'
#!/usr/bin/env bash
set -eu
project="$2"; shift 2
case "$1" in
  init) mkdir -p "$project/.git";;
  rev-parse) [ -f "$project/.git/test-head" ] && cat "$project/.git/test-head";;
  fetch) [ "$5" = 9de9c25f620ff7f1ce0fd5457d596052d5159596 ];;
  checkout) printf '%s\n' "$3" > "$project/.git/test-head";;
  status) [ "${TEST_DIRTY:-0}" != 1 ] || echo ' M user-file';;
  *) exit 1;;
esac
GIT
cat > "$fixture/bin/python3.13" <<'PYTHON'
#!/usr/bin/env bash
set -eu
[ "$1" != -c ] || exit 0
test "$1 $2" = '-m venv'
mkdir -p "$3/bin"
cat > "$3/bin/python" <<'PIP'
#!/usr/bin/env bash
exit "${TEST_PIP_EXIT:-0}"
PIP
cat > "$3/bin/hermes" <<'HERMES'
#!/usr/bin/env bash
echo 'Hermes test runtime'
HERMES
chmod +x "$3/bin/python" "$3/bin/hermes"
PYTHON
chmod +x "$fixture/bin/"*
installer="$repo/apps/cli/src/unix_hermes_install.sh"
# Dependency failures do not create an apparently installed launcher.
if TEST_PIP_EXIT=23 bash "$installer" > "$fixture/failure.log" 2>&1; then echo 'Expected dependency failure'; exit 1; fi
test ! -e "$HERMES_HOME/bin/hermes"
test ! -d "$HERMES_HOME/.mundusx-install.lock"
for os in Linux Darwin; do
  TEST_OS="$os" bash "$installer"
  test "$(cat "$HERMES_HOME/config.yaml")" = 'saved configuration'
  test ! -d "$HERMES_HOME/.mundusx-install.lock"
  # Git Bash may emulate symlinks as copies; isolate each platform's launcher.
  rm "$HERMES_HOME/bin/hermes"
done
if TEST_DIRTY=1 bash "$installer" > "$fixture/dirty.log" 2>&1; then echo 'Expected dirty-checkout refusal'; exit 1; fi
grep -q 'local edits' "$fixture/dirty.log"
printf 'different-revision\n' > "$HERMES_HOME/hermes-agent/.git/test-head"
if bash "$installer" > "$fixture/revision.log" 2>&1; then echo 'Expected revision mismatch refusal'; exit 1; fi
grep -q 'another revision' "$fixture/revision.log"
test "$(cat "$HERMES_HOME/config.yaml")" = 'saved configuration'
echo 'PASS: Linux/macOS setup flow, dependency failure, lock cleanup, configuration preservation, dirty/revision guards (mock dependencies).'
