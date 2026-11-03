# IDE-owned native tool workflow

OpenAI-compatible tool turns use the IDE's workflow by default. The contributor
runs inference; it does not execute the IDE's tools or claim filesystem authority.

The worker preserves the host's history, tool names, schemas, execution inputs,
and output budget. It does not substitute reads for proposed edits, fabricate
validation calls, or require a particular file-edit tool. Existing-file errors,
approvals, checkpoints and recovery belong to the IDE and its tool implementations.

Wire validation still rejects incomplete JSON, unknown functions, invalid required
arguments and violations of explicit tool choice. Missing terminal display labels
may be filled only when declared by the host schema; command and execution settings
are preserved. A malformed argument response has at most one schema repair attempt.
No failed call is executed or reported as successful by the contributor.

For diagnosing older integrations only, `OPENGPU_NATIVE_WORKFLOW=legacy` enables
the previous contributor workflow adapter. It is not the default API behavior.

Model architecture support, valid streaming and contributor isolation remain
required. An inference-only contributor never gains access to the caller's files.

When an IDE omits `max_tokens`, the worker forwards the job output budget to MLX (4,096 only if no job budget exists). Explicit caller limits are preserved. This avoids MLX’s 512-token default truncating ordinary file writes.

The public gateway assigns native IDE turns an 8,192-token output allowance only when the caller omits a limit. Explicit limits remain unchanged. The worker reports `finish_reason: length` as output truncation, releases no tool calls, and does not attempt a same-budget schema repair.


## Scoped app shutdown and latency diagnostics (worker 0.2.57)

VS Code owns local terminal execution. The worker rejects recognizable broad shared-runtime/browser termination calls before releasing any tool batch. This covers the reported wildcard Get-Process pipeline and common Stop-Process -Name / taskkill /IM / killall / pkill forms. PID-scoped commands and read-only discovery remain unchanged. One bounded model correction is allowed; the worker never invents a PID. This narrow syntactic check is not a shell sandbox and does not establish PID ownership; the host must enforce permissions and verify the target.

Each native inference attempt writes metadata to OPENGPU_HOME/native-call-timings.jsonl (default ~/.opengpu), bounded to about 2 MB. Fields include job_id, timestamp_ms, retry, model_call_ms, first_model_token_ms, and success. First-token timing includes runtime HTTP connection/prefill and only exists for streamed token progress. Model-call duration includes response validation. Prompts, tool arguments, outputs and credentials are omitted. Queue time and IDE tool/verification time remain null here because the inference worker cannot observe them. Control-plane submitted_at/assigned_at provide queue duration with one-second resolution. Contributor execution (assigned_at/completed_at) is not local VS Code execution. Do not add overlapping stage durations or present missing values as zero.

Global VS Code user instructions separately require app-specific discovery, targeted shutdown, real verification and concise lifecycle work. They apply on the next request that loads user instructions; they cannot rewrite completed chat history or guarantee model compliance. The worker guard requires the upgraded contributor binary.


## Persistent Mac worker service

`scripts/macos-worker-service.py` is the per-user launchd entry point used on the qualified Mac. Its LaunchAgent belongs in `~/Library/LaunchAgents/ai.opengpu.contributor.worker.plist`, with RunAtLoad enabled, KeepAlive SuccessfulExit=false, StartInterval=30 and ThrottleInterval=30. Configure ProgramArguments as the installed Python interpreter, this script, and the exact installed node-agent executable. Preserve HOME, OPENGPU_HOME and the GUI login context so Keychain identity remains available. Never launch a competing SSH-context daemon while this service is running.

Saved connected=false or invalid/missing configuration never starts the worker. Pause remains set in config and is enforced by the node agent. The launcher adopts an existing exact worker process without creating a duplicate, updates node-agent.pid for desktop visibility, and execs the worker when no matching process exists. Stop clears connected intent; a scheduled launcher check exits without restarting contribution. Auto-login requires a logged-in user; this is not a system daemon. Only migrate services after pausing admissions and verifying zero active slots. Existing model runtimes must be retained.

On 2026-11-01 the live idle-worker crash test recovered in 2.05 seconds, kept paused=true and had zero active slots. Login persistence is configured and the plist passes plutil; a physical reboot/logout was not performed.


## MLX prompt cache checkpoints (worker 0.2.58, runtime capability 11)

The batched MLX path retains an exact pre-generation checkpoint for tool-result turns, including the rendered tool catalog and leading system messages. A checkpoint leaves one token for decoding. This lets recurrent caches reuse the input prefix without trimming an assistant-completion state. Seeded/non-batched generation is not qualified by this change. Cache logging records token counts and retained bytes, never prompt content.

Prompt-cache admission reserves the same bytes used by the runtime: normally 4 GiB on a contribution budget of at least 32 GiB, increasing to 8 GiB only when cached model weights and runtime headroom leave at least 24 GiB. Smaller budgets disable retained prompt caching. This protects the 42 GiB Coder-Next configuration from an unconditional 8 GiB reservation while allowing the 16 GiB 30B model to retain a larger dense KV prefix. Weight quantization does not imply KV quantization.

Native timing records include prompt_tokens, cached_prompt_tokens and completion_tokens when reported; absent measurements remain null. A cold request still needs prefill. Restarting or switching models clears the cache. Context limits must reflect practical memory and prefill latency.

Validation on 2026-11-01: 22 Python runtime tests and 355 serial Mac Rust tests passed, followed by a release build. A 30B tool-result runtime probe measured 22.628 seconds cold, 0.164 seconds repeated (19,565 cached tokens), and 0.291 seconds with changed output (19,557 cached tokens). These are runtime probes, not end-to-end IDE success. The 30B VS Code startup test finished in 1m 46s but failed on shell syntax and a missing terminal mode; it must not be reported as a successful launch. Coder-Next comparison: the cold VS Code start recovered a shell error and completed 11 steps in 4m 6s; the warm start completed 3 steps in 47s, with independently observed HTTP 200 after 18.754s. Targeted shutdown completed in 53s and preserved the existing port-3000 server. The cold model call took 43.293s; subsequent turns reported cached token reuse. The VS Code user instruction now explicitly distinguishes PowerShell environment assignment from CMD syntax. Cold-start latency is still a limitation.


## Selected MLX model and greeting scope (worker 0.2.59)

The saved model selection is authoritative for active/warm advertisement. A persistent MLX contributor advertises only that selected model to the scheduler; other downloaded models remain in the local inventory. Previously a stale active flag in an installed manifest could let an automatic API alias select Coder-Next after the owner selected 30B, replacing the warm runtime and its prompt cache. An explicit missing selection no longer silently falls back to another active manifest.

Fresh VS Code chats also change VSCODE_TARGET_SESSION_LOG near the end of the system prompt, before Qwen renders tool definitions. Two measured requests had identical tool catalogs but different debug-log UUIDs; their common prefix was 9,702 tokens. Runtime diagnostics now record only the prior model-match flag and common-prefix token count, not prompt text.

Literal English greetings (hello, hi, hey and good morning/afternoon/evening) with automatic/no explicit function choice retain all messages and instructions but omit tool schemas and set tool_choice=none. The model still generates the answer. Explicit required/function choices, tool-result continuations and actionable requests such as "hello, start the server" remain unchanged. This is deliberately narrow, not a general small-talk classifier.

A proposed system-message split was rejected after live testing produced unsolicited server actions. That experiment was rolled back; original system-message layout is retained. During that test the app on port 3000 was restarted; its replacement listener and HTTP 200 were verified. Final greeting tests did not invoke tools or change that listener.

Validation on 2026-11-02: 357 serial native Mac Rust tests and 22 native Python tests passed. Actual VS Code cold hello completed in 13s, and hello in a separate fresh chat completed in 6s. Model processing was 9.106s cold and 1.228s warm (9,702 cached tokens); both used Qwen3-Coder-30B-A3B. These are measured examples, not a latency guarantee. Cold prompts and ordinary tool-rich coding requests still incur prefill. The fix is installed on the qualified Mac; downloadable installers have not been republished.

The subsequent read-only package.json smoke test completed in 19s but returned raw read_file markup as text rather than invoking the host tool. No mutation/terminal call occurred in that check. Therefore this release qualifies greeting scope and model-selection behavior, not the full 30B coding workflow.


## Network Activity termination (worker 0.2.60)

Network jobs now write bounded desktop metadata at execution start and completion. Assignment cancellation writes cancelled before releasing the slot; an unwound activity guard writes interrupted rather than leaving a running row. No prompt, output or raw error is persisted. The maintained desktop reads this feed and legacy jobCancelled terminal markers from both node-agent.log and contributor-worker.log on every refresh.

All 358 serial native Mac Rust tests passed, including cancellation/interruption privacy checks. Installed with OpenGPU Mac preview24 on 2026-11-02; the old cancelled chat is visible as cancelled and the Activity API reports zero active tasks. A first live disconnect probe completed before cancellation; automatic approval review blocked an additional combined cancellation probe without a detailed reason. No claim of a fresh end-to-end cancelled inference test is made.


## Live host-schema correction (worker 0.2.61)

The host-managed IDE path already forwards the caller's live tool schemas. A failed call previously lost its arguments before its one correction attempt, so the model could omit a mandatory field again or print tool markup as prose. Recovery now supplies the bounded rejected output and the exact relevant registered contract as private diagnostic data, preserves original messages and the tool catalog, and requires one regenerated call. Explicit function selections and tool_choice=none remain authoritative. Only the existing managed MLX guard receives its required-opening flag; no provider-specific flags are added to other backends. A second failure stops without releasing invalid calls or returning private argument diagnostics.

Missing run_in_terminal.mode is never guessed from a command or enum order. Only an explicit default on the live required mode property can fill it, and the entire completed call must validate. Explicit values, nulls, commands, timeouts and permission fields are not overwritten. Ordinary assistant text and fenced tool examples remain text. An unfenced, registered function block ending in the runtime closing tool marker is rejected as malformed protocol, not interpreted or executed directly.

Validation on 2026-11-03: 362 serial Windows Rust tests and 363 native Mac Rust tests passed. Tests cover missing-mode correction with original-command preservation, declared/invalid defaults, one-retry bounds, private diagnostic redaction, buffered streaming of only corrected tool calls, malformed protocol recovery and unchanged fenced examples. The native Mac release binary was installed as worker 0.2.61 after pausing and verifying zero active slots, with backup retained. Saved model, contribution intent and runtime files were preserved.

A public API probe returned a valid run_in_terminal call with command, goal, explanation and mode=sync in 5.649 seconds; the probe did not execute the command. Actual Windows VS Code default Chat with EHDA Public completed a read-only index.html title check in 14 seconds (3 UI steps). Metadata showed first validation failure then one successful correction for job chatcmpl-5c48e4a127c447b5b8e6ec8d8de41549, followed by the host-read result and final answer job chatcmpl-09991a936cc84828ac2261c3fb2b2936. The reported title matched local source: MB Water Bottles - Quality Solutions. No workspace edits or terminal actions were performed by this UI check. This qualifies schema recovery and that read workflow, not every coding task or third-party model.

The qualified public Mac contributor is live on this build. Other contributors and downloadable installers require the updated worker binary; this source commit alone does not upgrade them.
