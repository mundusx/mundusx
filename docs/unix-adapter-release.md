# macOS and Linux Hermes adapter release

These packages contain only `mundusx`, `mundusx-agent-server`, and setup/service
scripts. They do not install or enable an OpenGPU contributor node.

## Build and package manually

Build on the target OS/architecture where possible:

```sh
cargo build --release -p opengpu --bin mundusx -p mundusx-agent-server --bin mundusx-agent-server
bash scripts/package-unix-adapter.sh TARGET target/release artifacts/unix-adapters
```

Supported TARGET values are `aarch64-apple-darwin`, `x86_64-apple-darwin`,
`aarch64-unknown-linux-gnu`, and `x86_64-unknown-linux-gnu`. For cross-compilation,
pass `--target TARGET` and package `target/TARGET/release`. Cross-compilation is
not an installation test. No GitHub Actions are required.

## Native acceptance before publication

On a test account with Git and Python 3.11–3.13 installed:

1. Verify the archive SHA-256, extract it, then run `bash setup.sh` without sudo.
2. Verify `~/.local/bin/mundusx --version` and `~/.hermes/bin/hermes --version`.
3. Run `~/.local/bin/mundusx connect`, approve browser pairing, and create a Chat
   project. Ask Hermes to create and run a small program; confirm the file exists
   in the approved project and the command output is correct.
4. Restart the user service and confirm reconnection preserves the same identity.
5. Confirm no contributor process was started and existing configuration,
   conversations, project files, and credentials remain intact.

Setup refuses architecture mismatches, checksum failures, root execution,
modified Hermes checkouts, and conflicting non-managed launchers. Existing
Hermes revisions are not silently changed. Current binaries are backed up before
replacement. A missing systemd user session is reported with a manual
`mundusx connect` alternative; macOS uses launchd.

## Publication contract

Only after native acceptance, upload tarballs and matching `.sha256` assets to a
non-prerelease GitHub release in `mundusx/releases`:

- macOS tag: `adapter-macos-vVERSION`; assets:
  `mundusx-hermes-macos-arm64.tar.gz`, `mundusx-hermes-macos-x64.tar.gz`.
- Linux tag: `adapter-linux-vVERSION`; assets:
  `mundusx-hermes-linux-arm64.tar.gz`, `mundusx-hermes-linux-x64.tar.gz`.

Publish only architectures actually tested. Chat resolves the newest compatible
release independently for each architecture and refreshes within five minutes.
An unpublished architecture remains unavailable; it never falls back to Windows
or the old contributor package. Do not mark a cross-built package production-ready
without the native acceptance steps above.

## Current validation

Windows-hosted connector tests: 33 passed. Shell fixture tests passed for Linux
and macOS branches using mocked Git/Python dependencies, including dependency
failure, lock cleanup, configuration preservation, and dirty/revision guards.
Linux x64 and ARM64 release binaries cross-compiled and archives/checksums built.
Native Linux installation/pairing and macOS build/installation remain pending.
