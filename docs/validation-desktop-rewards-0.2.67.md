# Signed desktop rewards qualification — 0.2.67

The maintained CLI omitted the `desktop credits` adapter expected by the shared desktop. The desktop refused the operator-wide ledger fallback and displayed an unavailable balance. Historical balances were not deleted.

The restored command reuses the existing device identity, signs GET `/v1/contributor/credits`, validates the returned node and explicit `legacy_work_points` unit, and rejects missing/nonfinite totals. It never creates a replacement identity or reads the operator ledger. The control-plane endpoint derives the node only from the authenticated device header and returns whitelisted legacy totals. Desktop preview.26 labels these historical values as work points, separately from the USD-based AI wallet in Nodara Chat. No financial rows are migrated or repriced.

Executable CLI/worker source is tagged at `88a093c` for release 0.2.67. This qualification adds documentation only.

## Checks

- CLI suites: 272 CLI, 262 worker and 68 harness tests passed, with two existing interactive tests ignored. Bounded version/help smoke passed. A missing identity was rejected without creating settings or keys.
- Control plane: 524 tests passed, one existing ignored; endpoint signature, wrong-key, scope and metadata-exclusion checks passed. UAT deployment is `cf48de8b065c8fe0592b57f1b3a21d8225a0beaa` (radlertech/control-plane#33).
- Desktop: Go tests/vet and all 21 frontend tests passed. Current-device/unit validation, finite complete totals, correct point labels and stale-value clearing were checked. The ARM64 macOS and AMD64 Windows backends compiled as preview.26.
- Windows: release artifacts were checksum verified and installed in the active/canonical locations. Identity and contribution settings were compared and preserved. The installed preview.26 backend made the real signed request and returned 178 legacy work points for the Windows node; served frontend assets contained the correct labels. The visible desktop had been closed, and the verification helper ran headless.
- Mac: the release and native tests passed, but SSH at 192.168.2.45 timed out before installation began. Its matching app/CLI update is staged; live Mac signed-balance verification remains pending until the machine is reachable. No Mac update or live success is claimed here.

## Native release evidence

- [macOS](https://github.com/mundusx/mundusx/actions/runs/43927317845): passed.
- [Windows](https://github.com/mundusx/mundusx/actions/runs/43927318058): passed.
- [Linux](https://github.com/mundusx/mundusx/actions/runs/43927481707): both architectures passed on the exact tagged source, with the supported checks proving the 0.2.66 Vulkan runtime/adapter unchanged before checksum-verified reuse. This successful job supersedes the deliberately cancelled duplicate automatic Linux run 43927319934; no failing source test was skipped.

The server and desktop balance operations are read-only. The AI wallet conversion remains 1 AI credit = US$1; legacy character-based work points have no implied USD conversion.
