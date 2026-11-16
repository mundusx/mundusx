# Contributor activity qualification — 0.2.66

Official workers did not include the local desktop activity writer. The old implementation existed only in the desktop recovery archive, so official worker updates could leave both Activity pages without incoming-job metadata.

The maintained worker now writes the desktop-compatible, bounded `desktop-jobs.json` feed at claim/execution start, first streamed output and accepted terminal outcome. Cancellation and interrupted work remain distinct from success. Restart reconciles stale active rows. Local metadata failures do not fail inference. The feed excludes prompts, answers and raw errors.

## Verification

- Rust suites: 262 worker, 270 CLI and 68 harness tests passed; two existing interactive tests remain ignored. Bounded version and lifecycle smoke passed.
- Desktop reader: Go tests and vet passed; all 20 frontend tests passed. Existing tests cover worker states, active/history partitioning, safe rendering, interruption and exclusion of request data.
- macOS and Windows live requests traversed the normal control-plane job queue. Each local feed was observed as running, streaming and completed. Recorded model IDs matched each device configuration. Measured durations were 2,940 ms on the Mac and 5,474 ms on Windows. No native visual inspection was performed.
- Worker and CLI 0.2.66 were installed in active and canonical locations. The maintained Mac app bundle was also updated and its ad-hoc signature verified. Both identities and contribution settings were compared and preserved.
- Desktop packaging guards reject workers older than the activity-feed release. The Windows guard rejected verified 0.2.65 assets before building the desktop.

## Release evidence

Executable source is tagged at `2c2ee62bd0b8b4de7007ee2449c0180dd170cd78` for all three platform releases. This qualification document adds no executable changes.

- [macOS release gate](https://github.com/mundusx/mundusx/actions/runs/43904293452): passed on the full failed-job rerun. The first attempt failed in the existing CLI settings-lock test; the complete rerun passed without skipping that test or changing source.
- [Windows release gate](https://github.com/mundusx/mundusx/actions/runs/43904293813): passed.
- [Linux release gate](https://github.com/mundusx/mundusx/actions/runs/43904346912): both architectures passed on the exact tagged source. The unchanged Vulkan adapter/runtime was verified against release 0.2.65 with the supported runtime-reuse checks and manifest checksum. This job supersedes the deliberately cancelled duplicate automatic Linux run, 43904294656; that cancellation was not a failing source test.

Signed-manifest and packaged-asset verification passed in the platform release workflows. Installed artifact checksums matched the release manifests.

## Separate credit issue

The existing contributor balance endpoint still reports legacy character-based work points, using `ceil((prompt_chars + output_chars) / 400)`. This is separate from Nodara's agreed USD-based AI credits. The activity change does not migrate, reset or reprice those historical balances.
