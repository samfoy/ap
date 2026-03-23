#!/usr/bin/env python3
"""
ap-ralph-watchdog.py — Keeps a single Ralph pdd-to-code-assist loop alive.

- Starts Ralph if not running
- Restarts on stall (no new events for STALL_MINUTES)
- Logs to .monitor/watchdog.log
- Does NOT manage the backlog — that's a human job
"""

import subprocess
import time
import json
from pathlib import Path
from datetime import datetime

AP_DIR = Path.home() / "Projects/ap"
RALPH_LOG = AP_DIR / ".monitor-ralph.log"
LOG = AP_DIR / ".monitor/watchdog.log"
STALL_MINUTES = 30
CHECK_INTERVAL = 60
HEARTBEAT_CYCLES = 10

LOG.parent.mkdir(exist_ok=True)


def log(msg):
    ts = datetime.now().strftime("%H:%M:%S")
    line = f"[{ts}] {msg}"
    print(line, flush=True)
    with open(LOG, "a") as f:
        f.write(line + "\n")


def run(cmd, cwd=None):
    return subprocess.run(cmd, shell=True, capture_output=True, text=True, cwd=str(cwd or AP_DIR))


def ralph_running():
    r = run("pgrep -f 'ralph run' || true")
    return bool(r.stdout.strip())


def last_event_time():
    ralph_dir = AP_DIR / ".ralph"
    if not ralph_dir.exists():
        return None
    latest = None
    for f in sorted(ralph_dir.glob("events-*.jsonl")):
        try:
            mtime = f.stat().st_mtime
            if latest is None or mtime > latest:
                latest = mtime
        except Exception:
            pass
    return latest


def start_ralph():
    scratchpad = AP_DIR / ".ralph/agent/scratchpad.md"
    if scratchpad.exists():
        cmd = f"nohup ralph run -H builtin:pdd-to-code-assist --no-tui --continue >> {RALPH_LOG} 2>&1 &"
    else:
        cmd = f"nohup ralph run -H builtin:pdd-to-code-assist --no-tui >> {RALPH_LOG} 2>&1 &"
    run(cmd)
    time.sleep(3)
    if ralph_running():
        log("Ralph started")
        return True
    log("WARNING: Ralph failed to start")
    return False


def main():
    log("ap ralph watchdog started")

    if not (AP_DIR / "PROMPT.md").exists():
        log("No PROMPT.md found — nothing to run. Add a PROMPT.md and restart.")
        return

    if not ralph_running():
        start_ralph()

    cycle = 0
    while True:
        cycle += 1
        if cycle % HEARTBEAT_CYCLES == 0:
            log(f"♥ alive — ralph running: {ralph_running()}")

        if not ralph_running():
            log("Ralph not running — restarting")
            start_ralph()
        else:
            evt_time = last_event_time()
            if evt_time:
                stale_min = (time.time() - evt_time) / 60
                if stale_min > STALL_MINUTES:
                    log(f"STALL detected ({stale_min:.0f}m since last event) — restarting Ralph")
                    run("pkill -f 'ralph run' || true")
                    time.sleep(2)
                    start_ralph()

        time.sleep(CHECK_INTERVAL)


if __name__ == "__main__":
    main()
