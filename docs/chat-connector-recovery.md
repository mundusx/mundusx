# Chat connector recovery

The Unix installer configures a saved Chat connection to recover in the signed-in
user's session. It does not configure GPU inference services or change model support.

* Linux: a systemd user service and a desktop `mundusx://reconnect` URL handler.
  A desktop session with a working systemd user manager is required for automatic
  recovery. Headless machines can run `mundusx connect` in their own service setup.
* macOS: a LaunchAgent and a user Applications/MundusX Reconnect.app URL handler.
  The package includes the Chat connector and agent server. With no signed-in GUI
  user at package installation, run the registration command below after login.

For existing binaries, register without downloading them again:

```sh
INSTALL_DIR=/path/to/installed/binaries bash install.sh --configure-chat-service
```

On a new installation, run `mundusx connect` once and approve the computer in Chat.
Existing installations reuse `.mundusx/chat-connection.json`. Reconnect starts a
stopped service; it does not kill a running connector, rotate credentials, or download
an installer. Browsers may require permission to open the registered application.
The Unix process lock prevents a second connector using the same data directory.

Services start at user login and restart after failures. This does not keep a laptop
online while asleep or a user service alive after logout. The connector handles
network retries after connectivity returns. Automatic registration can be skipped
with `MUNDUSX_SKIP_CHAT_SERVICE=1`.

Native CI checks service generation, macOS URL bundle compilation, pairing
preservation, URI validation, and the Unix process lock. Actual browser launch,
sleep/wake, and logout/login recovery still need device acceptance testing before
claiming a released installer supports those scenarios.


## Durable task recovery

The connector keeps a transactional SQLite journal at
`$MUNDUSX_HOME/harness-tasks.db` (default `~/.mundusx/harness-tasks.db`).
Run `mundusx tasks` to inspect the latest 100 local records without contacting the
model or executing tools. `active` means no terminal result was journaled; it is
not proof that a process is still running. `report_pending` means execution ended
but server acknowledgement is outstanding. `acknowledged` means the server
accepted the report or already considers the task terminal.

The journal binds each task to its Chat URL, connector identity, canonical
workspace and session. Completed results are committed before uploading. After a
network failure or connector restart, pending reports are reconciled with the
server; cancelled tasks remain cancelled. A reclaimed task with a saved result
uploads that result without running Hermes again. Interrupted work without a
result requires a new server claim before execution; local history alone never
grants authority. Recovery can require waiting for the existing server lease.

Hermes writes project-specific checkpoints to
`<project>/.hermes/checkpoints/<task-id>.json`, recording completed boundaries and
in-flight tools. A tool whose completion was not recorded has an unknown outcome:
Hermes must inspect the current workspace before retrying it. This is not an
exactly-once guarantee for arbitrary commands or external side effects. The
Hermes session mapping is also saved when the session-start event arrives,
instead of waiting solely for the final response.

The SQLite journal uses WAL transactions and FULL durability. It stores tool
boundaries and terminal results, not streaming token deltas, full conversations,
or model prompts. Acknowledged records expire after 30 days when the journal
opens; unfinished records are retained. Each normal model turn still uses only
its relevant task/session context. Existing task process-group cleanup continues
to own test-server termination; the journal does not kill arbitrary processes
by a potentially reused PID.

Validation includes reopening the journal after an upload failure, exact saved
result replay, cancellation and terminal-state reconciliation, scope/workspace
isolation, unfinished-record retention, token exclusion, and interruption during
a tool call. Physical power-loss behavior and live production throughput have
not been benchmarked by these tests.
