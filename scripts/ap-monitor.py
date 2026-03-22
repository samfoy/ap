#!/usr/bin/env python3
"""
ap-monitor.py — Sequential development loop monitor using git worktrees.

Builds one backlog item at a time: spin a worktree + Ralph loop, wait for
LOOP_COMPLETE, clean merge into main, then move to the next item.

Robustness features:
- Persists active state to .monitor/state.json (survives restarts)
- Stall detection: restarts Ralph if no new events for STALL_MINUTES
- Pre-merge cleanup: strips .ralph/ state files + PROMPT.md before merging
  so ephemeral agent state never causes merge conflicts
- Git push after every merge
- Heartbeat log every N cycles
- Retry prompt generation up to 3 times
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
WORKTREES_DIR = Path.home() / "Projects/ap-worktrees"
BACKLOG = AP_DIR / "BACKLOG.md"
MEMORY = AP_DIR / ".monitor/memory.md"
LOG = AP_DIR / ".monitor/monitor.log"
STATE_FILE = AP_DIR / ".monitor/state.json"
CONFLICTS_DIR = AP_DIR / ".monitor/conflicts"

MAX_PARALLEL = 1          # sequential: one at a time
CHECK_INTERVAL = 30       # seconds between main loop ticks
STALL_MINUTES = 45        # restart Ralph if no new events for this long
HEARTBEAT_CYCLES = 10     # log "still alive" every N cycles
MAX_PROMPT_RETRIES = 3

MEMORY.parent.mkdir(exist_ok=True)
WORKTREES_DIR.mkdir(exist_ok=True)
CONFLICTS_DIR.mkdir(exist_ok=True)


# ── Logging ───────────────────────────────────────────────────────────────────

def log(msg):
    ts = datetime.now().strftime("%H:%M:%S")
    line = f"[{ts}] {msg}"
    print(line, flush=True)
    with open(LOG, "a") as f:
        f.write(line + "\n")


# ── State persistence ─────────────────────────────────────────────────────────

def load_state():
    """Load persisted active state: {title: {wt: str, started_at: float, last_event_at: float}}"""
    if STATE_FILE.exists():
        try:
            return json.loads(STATE_FILE.read_text())
        except Exception:
            pass
    return {}


def save_state(active):
    STATE_FILE.write_text(json.dumps(active, indent=2))


# ── Git helpers ───────────────────────────────────────────────────────────────

def run(cmd, cwd=None, **kwargs):
    return subprocess.run(
        cmd, shell=True, capture_output=True, text=True,
        cwd=str(cwd or AP_DIR), **kwargs
    )


def git_push():
    r = run("git push origin main")
    if r.returncode != 0:
        log(f"Push failed: {r.stderr[:200]}")
    return r.returncode == 0


def git_commit(msg, cwd=None):
    r = run(f'git add -A && git commit -m "{msg}" || true', cwd=cwd or AP_DIR)
    return r


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


# ── Worktree/branch naming ────────────────────────────────────────────────────

def slug(title):
    return re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-")[:40]


def worktree_path(title):
    return WORKTREES_DIR / slug(title)


def branch_name(title):
    return f"feature/{slug(title)}"


# ── Loop state detection ──────────────────────────────────────────────────────

def loop_completed(wt_path):
    scratchpad = wt_path / ".ralph/agent/scratchpad.md"
    if scratchpad.exists() and "LOOP_COMPLETE" in scratchpad.read_text():
        return True
    # Also check events file for loop.terminate
    for events_file in sorted((wt_path / ".ralph").glob("events-*.jsonl")):
        try:
            last_lines = events_file.read_bytes().decode("utf-8", errors="replace").strip().splitlines()
            for line in reversed(last_lines[-10:]):
                try:
                    d = json.loads(line)
                    if d.get("topic") in ("loop.terminate", "LOOP_COMPLETE"):
                        return True
                except Exception:
                    continue
        except Exception:
            continue
    return False


def last_event_time(wt_path):
    """Return unix timestamp of the most recent event, or None."""
    latest = None
    for events_file in sorted((wt_path / ".ralph").glob("events-*.jsonl")):
        try:
            mtime = events_file.stat().st_mtime
            if latest is None or mtime > latest:
                latest = mtime
        except Exception:
            continue
    return latest


def ralph_running(wt_path):
    """Check if a ralph process is running for this worktree."""
    r = run(f"pgrep -f 'ralph.*--no-tui' | head -1 || true")
    if not r.stdout.strip():
        return False
    # Check if the ralph process has this worktree's directory in its cwd
    for pid in r.stdout.strip().splitlines():
        cwd_check = run(f"lsof -p {pid.strip()} 2>/dev/null | grep cwd | grep '{wt_path.name}' || true")
        if cwd_check.stdout.strip():
            return True
    # Fallback: check if any ralph is running and the worktree has recent events (< 5 min)
    evt_time = last_event_time(wt_path)
    if evt_time and (time.time() - evt_time) < 300:
        return True
    return False


# ── Ralph lifecycle ───────────────────────────────────────────────────────────

def spawn_ralph(wt_path, title):
    log_file = wt_path / ".monitor-ralph.log"
    cmd = f"nohup ralph run -H builtin:pdd-to-code-assist --no-tui --continue >> {log_file} 2>&1 &"
    run(cmd, cwd=wt_path)
    time.sleep(3)
    # Quick sanity check — any ralph running at all?
    r = run("pgrep -f 'ralph run' || true")
    if r.stdout.strip():
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

Past completed work:
{run("git log --oneline -15").stdout.strip()}

Monitor memory:
{MEMORY.read_text() if MEMORY.exists() else "(none yet)"}

Write a detailed PROMPT.md for a Ralph pdd-to-code-assist loop to implement this goal.
Include: vision, technical requirements with specific Rust types/signatures, ordered implementation steps (each independently compilable), acceptance criteria.
End with: Output LOOP_COMPLETE when all acceptance criteria are met and the project builds clean.
Output only the PROMPT.md content."""

    r = subprocess.run(
        ["pi", "--provider", "amazon-bedrock",
         "--model", "us.anthropic.claude-sonnet-4-6",
         "--print", context],
        capture_output=True, text=True, env={**os.environ}, timeout=120
    )
    if r.returncode == 0 and r.stdout.strip():
        return r.stdout.strip()
    log(f"Prompt generation attempt {attempt} failed for '{title}': {r.stderr[:200]}")
    if attempt < MAX_PROMPT_RETRIES:
        time.sleep(10 * attempt)
        return generate_prompt(title, body, attempt + 1)
    return None


# ── Worktree setup ────────────────────────────────────────────────────────────

def setup_worktree(title):
    wt = worktree_path(title)
    branch = branch_name(title)
    if wt.exists():
        log(f"Worktree already exists: {wt}")
        return wt
    r = run(f"git worktree add {wt} -b {branch} main")
    if r.returncode != 0:
        # Branch may already exist from a prior run
        r2 = run(f"git worktree add {wt} {branch}")
        if r2.returncode != 0:
            log(f"Worktree creation failed: {r.stderr} / {r2.stderr}")
            return None
    log(f"Created worktree: {wt} ({branch})")
    return wt


# ── Merge ─────────────────────────────────────────────────────────────────────

def merge_worktree(title, wt_path):
    branch = branch_name(title)
    log(f"Merging {branch} into main...")

    # Strip ephemeral agent state files before merge to prevent conflicts
    ephemeral_patterns = [".ralph/", "PROMPT.md", ".monitor-ralph.log"]
    for pat in ephemeral_patterns:
        target = wt_path / pat
        if target.exists():
            run(f"rm -rf '{target}'")
    # Commit the cleanup on the feature branch
    r = run('git add -A && git commit -m "chore: strip ephemeral state before merge" || true', cwd=wt_path)

    r = run(f'git merge --no-ff {branch} -m "feat: merge {title}"')
    if r.returncode != 0:
        log(f"Merge conflict on {title}: {r.stderr[:300]}")
        run("git merge --abort || true")
        CONFLICTS_DIR.mkdir(parents=True, exist_ok=True)
        conflict_note = CONFLICTS_DIR / f"{slug(title)}.md"
        conflict_note.write_text(
            f"# Merge conflict: {title}\n\n"
            f"Branch: {branch}\n"
            f"Time: {datetime.now().isoformat()}\n\n"
            f"## Error\n```\n{r.stderr}\n```\n\n"
            f"## Manual fix\n"
            f"```bash\n"
            f"cd ~/Projects/ap\n"
            f"git merge --no-ff {branch}\n"
            f"# resolve conflicts\n"
            f"git add . && git commit\n"
            f"git push origin main\n"
            f"```\n"
        )
        log(f"Conflict info saved to {conflict_note}. Skipping {title}.")
        return False

    # Remove worktree and branch
    run(f"git worktree remove {wt_path} --force")
    run(f"git branch -d {branch} || git branch -D {branch} || true")
    log(f"Merged and cleaned up worktree for: {title}")
    git_push()
    return True


# ── Review + memory ───────────────────────────────────────────────────────────

def review(title, wt_path):
    try:
        commits = run("git log --oneline -8", cwd=wt_path).stdout.strip()
        r = subprocess.run(
            ["pi", "--provider", "amazon-bedrock",
             "--model", "us.anthropic.claude-sonnet-4-6",
             "--print",
             f"Review this git log for the ap Rust project. Goal was: {title}\n\n{commits}\n\nIn 2-3 sentences: did it land cleanly? Any gaps?"],
            capture_output=True, text=True, env={**os.environ}, timeout=60
        )
        return r.stdout.strip() if r.returncode == 0 else "(review unavailable)"
    except Exception as e:
        return f"(review error: {e})"


def update_memory(title, review_text, wt_path):
    commits = run("git log --oneline -6", cwd=wt_path).stdout.strip()
    with open(MEMORY, "a") as f:
        f.write(f"\n## {datetime.now().strftime('%Y-%m-%d %H:%M')} — {title}\n")
        f.write(f"Review: {review_text}\n")
        f.write(f"Commits:\n{commits}\n")


# ── Main loop ─────────────────────────────────────────────────────────────────

def main():
    log("ap monitor started (sequential mode)")

    # Load persisted state
    raw_state = load_state()
    active = {}  # title -> {wt: Path, started_at: float, last_restart_at: float}
    for title, info in raw_state.items():
        wt = Path(info["wt"])
        if wt.exists():
            active[title] = {
                "wt": wt,
                "started_at": info.get("started_at", time.time()),
                "last_restart_at": info.get("last_restart_at", 0),
            }
            log(f"Resuming: {title} in {wt}")

    # Also pick up any [~] items with existing worktrees not in state
    for item in get_backlog_items():
        if item["status"] == "~" and item["title"] not in active:
            wt = worktree_path(item["title"])
            if wt.exists():
                active[item["title"]] = {
                    "wt": wt,
                    "started_at": time.time(),
                    "last_restart_at": 0,
                }
                log(f"Resuming (from backlog): {item['title']} in {wt}")

    cycle = 0
    while True:
        try:
            cycle += 1
            if cycle % HEARTBEAT_CYCLES == 0:
                log(f"♥ alive — active: {list(active.keys()) or 'none'}")

            # ── Check for completed/stalled loops ──────────────────────────────
            for title in list(active.keys()):
                info = active[title]
                wt = info["wt"]

                if loop_completed(wt):
                    log(f"LOOP_COMPLETE: {title}")
                    rev = review(title, wt)
                    log(f"Review: {rev}")
                    update_memory(title, rev, wt)
                    if merge_worktree(title, wt):
                        set_item_status(title, "x")
                        git_commit(f"chore(monitor): complete {title}")
                        git_push()
                    del active[title]
                    save_state({k: {"wt": str(v["wt"]), "started_at": v["started_at"], "last_restart_at": v["last_restart_at"]} for k, v in active.items()})
                    continue

                # Stall detection
                evt_time = last_event_time(wt)
                if evt_time:
                    stale_minutes = (time.time() - evt_time) / 60
                    if stale_minutes > STALL_MINUTES:
                        since_restart = (time.time() - info.get("last_restart_at", 0)) / 60
                        if since_restart > STALL_MINUTES:
                            log(f"STALL detected for {title} ({stale_minutes:.0f}m since last event) — restarting Ralph")
                            spawn_ralph(wt, title)
                            active[title]["last_restart_at"] = time.time()
                            save_state({k: {"wt": str(v["wt"]), "started_at": v["started_at"], "last_restart_at": v["last_restart_at"]} for k, v in active.items()})

            # ── Spawn new loops if slots available ─────────────────────────────
            if len(active) < MAX_PARALLEL:
                items = get_backlog_items()
                pending = [i for i in items if i["status"] == " "]
                slots = MAX_PARALLEL - len(active)

                for item in pending[:slots]:
                    title = item["title"]
                    log(f"Starting: {title}")
                    set_item_status(title, "~")

                    wt = setup_worktree(title)
                    if not wt:
                        set_item_status(title, " ")
                        continue

                    # Generate PROMPT.md (with retries)
                    if not (wt / "PROMPT.md").exists():
                        prompt = generate_prompt(title, item["body"])
                        if not prompt:
                            log(f"Failed to generate prompt for {title} after {MAX_PROMPT_RETRIES} attempts — skipping")
                            set_item_status(title, " ")
                            run(f"git worktree remove {wt} --force || true")
                            continue
                        (wt / "PROMPT.md").write_text(prompt)
                        agents = AP_DIR / "AGENTS.md"
                        if agents.exists():
                            (wt / "AGENTS.md").write_text(agents.read_text())
                        run(f'git add PROMPT.md AGENTS.md && git commit -m "chore: init {title}" || true', cwd=wt)

                    git_commit(f"chore(monitor): start {title}")

                    if spawn_ralph(wt, title):
                        active[title] = {
                            "wt": wt,
                            "started_at": time.time(),
                            "last_restart_at": 0,
                        }
                        save_state({k: {"wt": str(v["wt"]), "started_at": v["started_at"], "last_restart_at": v["last_restart_at"]} for k, v in active.items()})
                    else:
                        set_item_status(title, " ")

            # ── Check for all done ─────────────────────────────────────────────
            if not active:
                pending = [i for i in get_backlog_items() if i["status"] == " "]
                if not pending:
                    log("🎉 All backlog items complete!")
                    break

        except Exception as e:
            log(f"Monitor error: {e}\n{traceback.format_exc()[:500]}")

        time.sleep(CHECK_INTERVAL)


if __name__ == "__main__":
    main()
