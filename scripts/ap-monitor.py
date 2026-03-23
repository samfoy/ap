#!/usr/bin/env python3
"""
ap-monitor.py — Sequential development loop monitor (no worktrees).

Builds one backlog item at a time directly on main:
1. Generate PROMPT.md in the ap repo
2. Spawn Ralph loop
3. Wait for LOOP_COMPLETE
4. Commit, push, move to next item
"""

import json
import subprocess
import time
import re
import os
import traceback
from pathlib import Path
from datetime import datetime

AP_DIR = Path.home() / "Projects/ap"
BACKLOG = AP_DIR / "BACKLOG.md"
MEMORY = AP_DIR / ".monitor/memory.md"
LOG = AP_DIR / ".monitor/monitor.log"

CHECK_INTERVAL = 30       # seconds between main loop ticks
STALL_MINUTES = 45        # restart Ralph if no new events for this long
HEARTBEAT_CYCLES = 10     # log "still alive" every N cycles
MAX_PROMPT_RETRIES = 3

MEMORY.parent.mkdir(exist_ok=True)
LOG.parent.mkdir(exist_ok=True)


# ── Logging ───────────────────────────────────────────────────────────────────

def log(msg):
    ts = datetime.now().strftime("%H:%M:%S")
    line = f"[{ts}] {msg}"
    print(line, flush=True)
    with open(LOG, "a") as f:
        f.write(line + "\n")


# ── Git helpers ───────────────────────────────────────────────────────────────

def run(cmd, cwd=None, **kwargs):
    return subprocess.run(
        ["zsh", "-il", "-c", cmd], capture_output=True, text=True,
        cwd=str(cwd or AP_DIR), **kwargs
    )


def git_push():
    r = run("git push origin main")
    if r.returncode != 0:
        log(f"Push failed: {r.stderr[:200]}")
    return r.returncode == 0


def git_commit(msg, cwd=None):
    run(f'git add -A && git commit -m "{msg}" || true', cwd=cwd or AP_DIR)


# ── Backlog helpers ───────────────────────────────────────────────────────────

def get_backlog_items():
    text = BACKLOG.read_text()
    items = []
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        m = re.match(r"(\d+)\. \[(.)\] \*\*(.+?)\*\*(.*)", lines[i])
        if m:
            num, status, title, rest = m.groups()
            body = lines[i]
            j = i + 1
            while j < len(lines) and (lines[j].startswith("    ") or lines[j] == ""):
                body += "\n" + lines[j]
                j += 1
            items.append({"num": int(num), "status": status, "title": title, "body": body})
            i = j
        else:
            i += 1
    return items


def set_item_status(title, status):
    text = BACKLOG.read_text()
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if f"**{title}**" in line and re.search(r"\[.\]", line):
            lines[i] = re.sub(r"\[.\]", f"[{status}]", line)
            break
    BACKLOG.write_text("\n".join(lines))


def slug(title):
    return re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-")[:40]


# ── Ralph helpers ─────────────────────────────────────────────────────────────

RALPH_LOG = AP_DIR / ".monitor-ralph.log"


def loop_completed():
    """Returns True only if LOOP_COMPLETE was genuinely accepted (not just loop.terminate on failure)."""
    scratchpad = AP_DIR / ".ralph/agent/scratchpad.md"
    if scratchpad.exists() and "LOOP_COMPLETE" in scratchpad.read_text():
        return True
    for events_file in sorted((AP_DIR / ".ralph").glob("events-*.jsonl")):
        try:
            lines = events_file.read_bytes().decode("utf-8", errors="replace").strip().splitlines()
            for line in reversed(lines[-20:]):
                try:
                    d = json.loads(line)
                    topic = d.get("topic", "")
                    # loop.terminate means ralph stopped but NOT due to successful LOOP_COMPLETE
                    # Only treat as complete if we see an actual LOOP_COMPLETE event
                    if topic == "LOOP_COMPLETE":
                        return True
                except Exception:
                    continue
        except Exception:
            continue
    return False


def loop_failed():
    """Returns True if ralph terminated due to failures (not successful completion)."""
    for events_file in sorted((AP_DIR / ".ralph").glob("events-*.jsonl")):
        try:
            lines = events_file.read_bytes().decode("utf-8", errors="replace").strip().splitlines()
            for line in reversed(lines[-20:]):
                try:
                    d = json.loads(line)
                    topic = d.get("topic", "")
                    payload = d.get("payload", {})
                    reason = payload.get("reason", "") if isinstance(payload, dict) else ""
                    if topic == "loop.terminate" and reason in (
                        "consecutive_failures", "loop_thrashing", "stale_loop",
                        "validation_failure", "max_iterations"
                    ):
                        return True
                except Exception:
                    continue
        except Exception:
            continue
    return False


def last_event_time():
    latest = None
    ralph_dir = AP_DIR / ".ralph"
    if not ralph_dir.exists():
        return None
    for events_file in sorted(ralph_dir.glob("events-*.jsonl")):
        try:
            mtime = events_file.stat().st_mtime
            if latest is None or mtime > latest:
                latest = mtime
        except Exception:
            continue
    return latest


def ralph_running():
    r = run("pgrep -f 'ralph run' || true")
    return bool(r.stdout.strip())


def check_aws_credentials(max_retries=3, retry_delay=10):
    """Warm up AWS credentials, retrying if stale. Returns True if ready."""
    for attempt in range(1, max_retries + 1):
        r = run(f"{AP_DIR}/ap/target/release/ap --prompt 'ping' 2>&1 | head -1")
        output = (r.stdout + r.stderr).strip()
        if "AWS error" in output or "service error" in output or "ExpiredToken" in output:
            log(f"AWS credentials stale (attempt {attempt}/{max_retries}) — waiting {retry_delay}s")
            # Touch the profile to trigger SSO refresh
            run("aws sts get-caller-identity --profile openclaw-bedrock")
            time.sleep(retry_delay)
        else:
            log("AWS credentials OK")
            return True
    log("WARNING: AWS credentials still stale after retries — proceeding anyway")
    return False


def spawn_ralph(title):
    # Warm up AWS credentials before spawning
    check_aws_credentials()

    # Clear stale ralph state from prior loop (including stale lock files)
    stale = AP_DIR / ".ralph"
    if stale.exists():
        run(f"rm -rf '{stale}'")
    # Extra safety: remove any orphaned lock left by a dead PID
    lock = AP_DIR / ".ralph" / "loop.lock"
    if lock.exists():
        lock.unlink(missing_ok=True)

    scratchpad = AP_DIR / ".ralph/agent/scratchpad.md"
    if scratchpad.exists():
        cmd = f"nohup zsh -il -c 'ralph run --no-tui --backend ap --idle-timeout 300 -H builtin:pdd-to-code-assist --continue >> {RALPH_LOG} 2>&1' &"
    else:
        cmd = f"nohup zsh -il -c 'ralph run --no-tui --backend ap --idle-timeout 300 -H builtin:pdd-to-code-assist >> {RALPH_LOG} 2>&1' &"
    run(cmd)
    time.sleep(10)
    if ralph_running():
        log(f"Ralph running for {title}")
        return True
    log(f"WARNING: Ralph may not have started for {title}")
    return False


# ── Prompt generation ─────────────────────────────────────────────────────────

def generate_prompt(title, body, attempt=1):
    context = f"""You are helping build `ap`, a Rust/ratatui AI coding agent (~/Projects/ap/ap/).
The project uses functional-first Rust: pure functions, immutable data, iterator chains, Middleware chain.
Core types: Conversation (immutable), turn() -> Result<(Conversation, Vec<TurnEvent>)>, Middleware chain.

The next development goal is: **{title}**

Backlog item spec:
{body}

Past completed work (recent git log):
{run("git log --oneline -10").stdout.strip()}

Write a detailed PROMPT.md for a Ralph pdd-to-code-assist loop to implement this goal.
Include: vision, technical requirements with specific Rust types/signatures, ordered implementation steps (each independently compilable), acceptance criteria.
End with: Output LOOP_COMPLETE when all acceptance criteria are met and the project builds clean.
Output only the PROMPT.md content."""

    ap_bin = str(AP_DIR / "ap" / "target" / "release" / "ap")
    r = subprocess.run(
        ["zsh", "-l", "-c", f"{ap_bin} --prompt {__import__('shlex').quote(context)}"],
        capture_output=True, text=True, timeout=300
    )
    if r.returncode == 0 and r.stdout.strip():
        raw = r.stdout.strip()
        # Strip any preamble before the first markdown heading
        if '\n# ' in raw:
            raw = raw[raw.index('\n# ') + 1:]
        elif raw.startswith('# '):
            pass  # clean already
        else:
            # Last resort: find first '#' line
            lines = raw.splitlines()
            for i, line in enumerate(lines):
                if line.startswith('#'):
                    raw = '\n'.join(lines[i:])
                    break
        return raw
    log(f"Prompt generation attempt {attempt} failed for '{title}': {r.stderr[:200]}")
    if attempt < MAX_PROMPT_RETRIES:
        time.sleep(10 * attempt)
        return generate_prompt(title, body, attempt + 1)
    return None


# ── Review ────────────────────────────────────────────────────────────────────

def review(title):
    try:
        commits = run("git log --oneline -8").stdout.strip()
        ap_bin = str(AP_DIR / "ap" / "target" / "release" / "ap")
        review_prompt = f"Review this git log for the ap Rust project. Goal was: {title}\n\n{commits}\n\nIn 2-3 sentences: did it land cleanly? Any gaps?"
        r = subprocess.run(
            ["zsh", "-l", "-c", f"{ap_bin} --prompt {__import__('shlex').quote(review_prompt)}"],
            capture_output=True, text=True, timeout=60
        )
        return r.stdout.strip() if r.returncode == 0 else "(review unavailable)"
    except Exception as e:
        return f"(review error: {e})"


def update_memory(title, review_text):
    commits = run("git log --oneline -6").stdout.strip()
    with open(MEMORY, "a") as f:
        f.write(f"\n## {datetime.now().strftime('%Y-%m-%d %H:%M')} — {title}\n")
        f.write(f"Review: {review_text}\n")
        f.write(f"Commits:\n{commits}\n")


# ── Main loop ─────────────────────────────────────────────────────────────────

def main():
    log("ap monitor started (direct-on-main mode)")
    check_aws_credentials()

    current_title = None
    current_started_at = None
    last_restart_at = 0

    # Resume any in-progress item
    for item in get_backlog_items():
        if item["status"] == "~":
            current_title = item["title"]
            current_started_at = time.time()
            log(f"Resuming in-progress item: {current_title}")
            # Regenerate PROMPT.md if missing
            if not (AP_DIR / "PROMPT.md").exists():
                log(f"PROMPT.md missing — regenerating for {current_title}")
                prompt = generate_prompt(current_title, item["body"])
                if prompt:
                    (AP_DIR / "PROMPT.md").write_text(prompt)
                    git_commit(f"chore: regenerate PROMPT.md for {current_title}")
            # Restart Ralph if not running
            if not ralph_running():
                spawn_ralph(current_title)
            break

    cycle = 0
    while True:
        try:
            cycle += 1
            if cycle % HEARTBEAT_CYCLES == 0:
                log(f"♥ alive — active: {[current_title] if current_title else 'none'}")

            if current_title:
                # Check for completion
                if loop_completed():
                    log(f"LOOP_COMPLETE: {current_title}")
                    rev = review(current_title)
                    log(f"Review: {rev}")
                    update_memory(current_title, rev)

                    # Verify something actually landed in src/
                    # Check if any src/ changes landed since the init commit for this item
                    init_sha = run(f"git log --oneline --grep='chore: init {current_title}' --format='%H' | head -1").stdout.strip()
                    if init_sha:
                        src_changes = run(f"git diff {init_sha} HEAD --name-only -- 'ap/src/'").stdout.strip()
                    else:
                        src_changes = run("git diff HEAD~3 HEAD --name-only -- 'ap/src/'").stdout.strip()
                    if not src_changes:
                        log(f"⚠️  No src/ changes detected for {current_title} — marking failed, requeueing")
                        set_item_status(current_title, " ")
                        git_commit(f"chore(monitor): requeue {current_title} (no src changes)")
                        current_title = None
                        current_started_at = None
                        last_restart_at = 0
                        time.sleep(10)
                        continue

                    # Clean up ephemeral state, commit, push
                    for pat in [".ralph/", "PROMPT.md", ".monitor-ralph.log"]:
                        p = AP_DIR / pat
                        if p.exists():
                            run(f"rm -rf '{p}'")

                    set_item_status(current_title, "x")
                    git_commit(f"chore(monitor): complete {current_title}")
                    git_push()

                    current_title = None
                    current_started_at = None
                    last_restart_at = 0
                elif loop_failed():
                    log(f"⚠️  Ralph terminated with failures for {current_title} — restarting loop")
                    run(f"rm -rf '{AP_DIR / '.ralph'}'")
                    last_restart_at = 0
                    spawn_ralph(current_title)
                    last_restart_at = time.time()
                else:
                    # Stall detection
                    evt_time = last_event_time()
                    if evt_time:
                        stale_min = (time.time() - evt_time) / 60
                        since_restart = (time.time() - last_restart_at) / 60
                        if stale_min > STALL_MINUTES and since_restart > STALL_MINUTES:
                            log(f"STALL detected for {current_title} ({stale_min:.0f}m) — restarting Ralph")
                            spawn_ralph(current_title)
                            last_restart_at = time.time()

            if not current_title:
                # Pick next pending item
                items = get_backlog_items()
                pending = [i for i in items if i["status"] == " "]

                if not pending:
                    log("🎉 All backlog items complete!")
                    break

                item = pending[0]
                current_title = item["title"]
                current_started_at = time.time()
                last_restart_at = 0
                log(f"Starting: {current_title}")
                set_item_status(current_title, "~")

                # Generate PROMPT.md
                prompt = generate_prompt(current_title, item["body"])
                if not prompt:
                    log(f"Failed to generate prompt for {current_title} — skipping")
                    set_item_status(current_title, " ")
                    current_title = None
                    time.sleep(CHECK_INTERVAL)
                    continue

                (AP_DIR / "PROMPT.md").write_text(prompt)
                git_commit(f"chore: init {current_title}")
                git_push()

                spawn_ralph(current_title)
                # Don't clear current_title on failed spawn — loop will retry ralph on next tick

        except Exception as e:
            log(f"Monitor error: {e}\n{traceback.format_exc()[:500]}")

        time.sleep(CHECK_INTERVAL)


if __name__ == "__main__":
    main()
