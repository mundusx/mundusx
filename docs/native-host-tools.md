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
