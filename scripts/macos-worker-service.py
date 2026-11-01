#!/usr/bin/env python3
"""Per-user launchd entry point. Never override saved contribution intent."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def should_run(config):
    # Missing/corrupt intent must never start contribution. Pause is retained in
    # config; an agent may stay connected while paused but cannot accept jobs.
    return config.get("connected") is True


def read_intent(home):
    try:
        return json.loads((home / "config.json").read_text())
    except (OSError, ValueError):
        return {}


def existing_worker(output, executable):
    expected = str(executable) + " run"
    for line in output.splitlines():
        fields = line.strip().split(None, 1)
        if len(fields) == 2 and fields[1] == expected:
            return int(fields[0])
    return None


def record_pid(home, pid):
    temporary = home / "node-agent.pid.launchd"
    temporary.write_text(str(pid))
    temporary.chmod(0o600)
    os.replace(temporary, home / "node-agent.pid")


def main():
    home = Path(os.environ.get("OPENGPU_HOME", str(Path.home() / ".opengpu")))
    executable = Path(sys.argv[1])
    if not executable.is_file():
        raise RuntimeError("Configured worker executable is missing")
    while should_run(read_intent(home)):
        output = subprocess.check_output(["/bin/ps", "-axo", "pid=,command="], text=True)
        existing = existing_worker(output, executable)
        if existing is not None:
            # Adopt existing GUI/CLI worker visibility, without spawning a second
            # worker or changing that process's identity or runtime.
            record_pid(home, existing)
            time.sleep(5)
            continue
        record_pid(home, os.getpid())
        os.execv(str(executable), [str(executable), "run"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
