# Measured contributor earnings qualification: 0.2.69

Executable source: `32327592a753063e10840a096670ed2fb284a621`.

Local Windows checks: 272 CLI, 264 worker and 68 harness tests passed; two existing
interactive tests ignored. Bounded version/help, Activity-count and exact claimed
assignment receipt smoke tests passed. The worker signs graph node, attempt and
assignment timestamp from the actual claimed job. It never invents measurements.

All native release gates passed:
- macOS run 43957430879, official cli-macos-v0.2.69.
- Windows run 43957431498, official cli-windows-v0.2.69.
- Linux supported verified-runtime reuse run 43957563297, cli-linux-v0.2.69.
  The redundant automatic Linux run 43957432115 was cancelled before packaging.
  The passing replacement built the exact same source, verified unchanged engine
  and adapter code, and checked the signed 0.2.67 runtime manifest/checksums.
  No compatibility gate or test was skipped to publish a different executable.

UAT control plane PR34 is deployed at 23087ca54335bf49e2a044f22d65f7804c984f95.
Railway Redis is running in radlertech UAT; validated GitHub pricing is cached for
2,592,000 seconds. Unsigned cache reset returned 401; operator reset returned 200.
The next request populated the cache; a later request retained its decreasing
TTL, proving cache reuse. Historical model-price snapshots remain immutable.

Owner-authorized UAT reset archived 235 old reward entries for the Mac and Windows
nodes, preserving audit records while clearing active balances. These legacy
character-based points are never converted to AI credits. One AI credit is US$1.
Completed measured requests earn full catalog Nodara input/output cost, already
10% below the cited market rate, without another discount or extra multiplier.

Mac CLI/worker 0.2.69 and desktop preview.28 were installed and signature-verified.
Identity, linked account, selected model and contribution settings were preserved.
Live Activity showed running, streaming and completed states, with 396 measured
input and 70 output tokens. The NUMERIC earning was exactly 0.000041958000 credits.
The installed desktop returned the same balance/counts and removed old point cards.

The real public Nodara UAT API returned HTTP 200 with 395 input and 86 output
tokens. The contributor receipt earned exactly 0.000045783000 credits at Qwen3
Coder rates of 0.063 input / 0.243 output USD per million tokens. Server-generated
OpenAI-compatible response IDs differ from underlying contributor job IDs. Both
measured usage and the priced receipt were checked without recording chat content.

Windows official CLI/worker 0.2.69 and desktop preview.28 are installed; identity,
model and settings are preserved. Automatic review blocked a programmatic worker
restart, so the user was asked to resume it in the opened native app. Final live
Windows metering verification is pending that manual resume.

This qualification covers node earnings, not consumer wallet debits, transfers
or withdrawals. No production deployment was performed.
