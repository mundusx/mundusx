# Native coding task progress

The worker reconstructs a compact progress record from the current tagged user request and matched assistant tool calls/host results. It does not add a database, execute tools itself, or share observations between users. The client's conversation remains the source of context.

The record identifies observed source ranges by call ID and content digest, confirmed source writes, and the most recent completed validation command and status. A digest identifies observed bytes; it is not a filesystem version or a claim that external edits cannot occur. Unknown operations and terminal execution invalidate source-read observations. Source changes and environment operations invalidate previous validation results. Failed, truncated, wrapped or incomplete observations do not establish complete coverage.

Before inference, the model receives this compact record and guidance to continue from existing evidence. Before releasing a tool call, the worker rejects a covered duplicate read or an identical completed validation without a relevant intervening operation. One bounded correction attempt can choose the next useful action; the rejected call is never executed or streamed to the host. Missing ranges, changed state, incomplete terminal output and new user requests remain inspectable.

For tasks explicitly requiring automated tests or assertions, editor diagnostics, syntax-only validation, printed example results and zero-test runs do not establish completion. The worker requires a successful host exit and a recognized nonempty test-runner report. Unknown reporter formats remain unverified. Functional coverage and application correctness still require appropriate tests; a runner summary alone cannot prove that every requested behavior was tested.

Planning stays in the client's registered planning tool and conversation. Its completion labels are model-reported state, not evidence that builds or tests ran. Compaction that removes tool evidence requires fresh observations rather than inventing retained state.
