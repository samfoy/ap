#!/usr/bin/env python3
"""
ap-monitor.py — Parallel development loop monitor using git worktrees.

Picks up to MAX_PARALLEL independent backlog items, spins each in its own
worktree + Ralph loop, watches all in parallel, merges on completion.
"""

import json
import subprocess
import time
import re
import os
from pathlib import Path
from datetime import datetime

AP_DIR = Path.home() / "Projects/ap"
WORKTREES_DIR = Path.home() / "Projects/ap-worktrees"
BACKLOG = AP_DIR / "BACKLOG.md"
MEMORY = AP_DIR / ".monitor/memory.md"
LOG = AP_DIR / ".monitor/monitor.log"

MAX_PARALLEL = 3
CHECK_INTERVAL = 30  # seconds

# Items that touch shared pipeline core — must be sequential
SEQUENTIAL_TAGS = ["turn.rs", "types.rs", "middleware", "provider"]

MEMORY.parent.mkdir(exist_ok=True)
WORKTREES_DIR.mkdir(exist_ok=True)


def log(msg):
    ts = datetime.now().strftime("%H:%M:%S")
    line = f"[{ts}] {msg}"
    print(line)
    with open(LOG, "a") as f:
        f.write(line + "\n")


def run(cmd, cwd=None, **kwargs):
    return subprocess.run(
        cmd, shell=True, capture_output=True, text=True,
        cwd=str(cwd or AP_DIR), **kwargs
    )


def get_backlog_items():
    """Return list of (number, title, body, status) for all items."""
    text = BACKLOG.read_text()
    items = []
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        m = re.match(r"(\d+)\. \[(.)\] \*\*(.+?)\*\*(.*)", lines[i])
        if m:
            num, status, title, rest = m.groups()
            body = lines[i]
            # Grab continuation lines
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


def worktree_path(title):
    return WORKTREES_DIR / slug(title)


def branch_name(title):
    return f"feature/{slug(title)}"


def loop_completed(wt_path):
    scratchpad = wt_path / ".ralph/agent/scratchpad.md"
    if not scratchpad.exists():
        return False
    return "LOOP_COMPLETE" in scratchpad.read_text()


def loop_running(wt_path):
    r = run(f"pgrep -f 'ralph run.*{wt_path.name}' || true", cwd=wt_path)
    return bool(r.stdout.strip())


def generate_prompt(title, body):
    """Ask pi (bedrock) to write a PROMPT.md for the item."""
    context = f"""You are helping build `ap`, a Rust/ratatui AI coding agent (~/Projects/ap/ap/).
The project uses functional-first Rust: pure functions, immutable data, iterator chains, Middleware chain.
Core types: Conversation (immutable), turn() -> Result<(Conversation, Vec<TurnEvent>)>, Middleware chain.

The next development goal is: **{title}**

Backlog item spec:
{body}

Past completed work:
{run("git log --oneline -10").stdout.strip()}

Monitor memory:
{MEMORY.read_text() if MEMORY.exists() else "(none yet)"}

Write a detailed PROMPT.md for a Ralph pdd-to-code-assist loop to implement this goal.
Include: vision, technical requirements with specific Rust types/signatures, ordered implementation steps (each independently compilable), acceptance criteria.
End with: Output LOOP_COMPLETE when all acceptance criteria are met and the project builds clean.
Output only the PROMPT.md content."""

    r = subprocess.run(
        ["pi", "--provider", "amazon-bedrock", "--model", "us.anthropic.claude-sonnet-4-6", "--print", context],
        capture_output=True, text=True, env={**os.environ}
    )
    if r.returncode == 0 and r.stdout.strip():
        return r.stdout.strip()
    log(f"Pi failed: {r.stderr[:200]}")
    return None


def setup_worktree(title):
    """Create git worktree + branch for item."""
    wt = worktree_path(title)
    branch = branch_name(title)
    if wt.exists():
        log(f"Worktree already exists: {wt}")
        return wt
    r = run(f"git worktree add {wt} -b {branch} main")
    if r.returncode != 0:
        log(f"Worktree creation failed: {r.stderr}")
        return None
    log(f"Created worktree: {wt} ({branch})")
    return wt


def spawn_ralph(wt_path):
    """Start Ralph loop in worktree background."""
    log_file = wt_path / ".monitor-ralph.log"
    cmd = f"nohup ralph run -H builtin:pdd-to-code-assist --no-tui --continue > {log_file} 2>&1 &"
    run(cmd, cwd=wt_path)
    time.sleep(2)
    r = run(f"pgrep -f 'ralph run' || true", cwd=wt_path)
    if r.stdout.strip():
        log(f"Ralph running in {wt_path.name} (PID {r.stdout.strip().split()[0]})")
        return True
    log(f"WARNING: Ralph may not have started in {wt_path.name}")
    return False


def merge_worktree(title, wt_path):
    """Merge feature branch into main and remove worktree."""
    branch = branch_name(title)
    log(f"Merging {branch} into main...")
    r = run(f"git merge --no-ff {branch} -m 'feat: merge {title}'")
    if r.returncode != 0:
        log(f"Merge conflict on {title}: {r.stderr[:200]}")
        log("Attempting rebase...")
        run(f"git checkout {branch}", cwd=wt_path)
        r2 = run(f"git rebase main", cwd=wt_path)
        if r2.returncode != 0:
            log(f"Rebase failed — manual intervention needed for {title}")
            return False
        run(f"git checkout main")
        run(f"git merge --ff-only {branch}")

    run(f"git worktree remove {wt_path} --force")
    run(f"git branch -d {branch} || true")
    log(f"Merged and cleaned up worktree for: {title}")
    return True


def review(title, wt_path):
    commits = run("git log --oneline -6", cwd=wt_path).stdout.strip()
    r = subprocess.run(
        ["pi", "--provider", "amazon-bedrock", "--model", "us.anthropic.claude-sonnet-4-6",
         "--print", f"Review this git log for the ap Rust project. Goal was: {title}\n\n{commits}\n\nIn 2-3 sentences: did it land cleanly? Any gaps?"],
        capture_output=True, text=True, env={**os.environ}
    )
    return r.stdout.strip() if r.returncode == 0 else "(review unavailable)"


def update_memory(title, review_text, wt_path):
    commits = run("git log --oneline -6", cwd=wt_path).stdout.strip()
    with open(MEMORY, "a") as f:
        f.write(f"\n## {datetime.now().strftime('%Y-%m-%d %H:%M')} — {title}\n")
        f.write(f"Review: {review_text}\n")
        f.write(f"Commits:\n{commits}\n")


def main():
    log("ap monitor started (parallel worktree mode)")
    # active: dict of title -> worktree path
    active = {}

    # Resume any in-progress items
    for item in get_backlog_items():
        if item["status"] == "~":
            wt = worktree_path(item["title"])
            if wt.exists():
                active[item["title"]] = wt
                log(f"Resuming: {item['title']} in {wt}")

    while True:
        try:
            # Check completed loops
            for title in list(active.keys()):
                wt = active[title]
                if loop_completed(wt):
                    log(f"LOOP_COMPLETE: {title}")
                    rev = review(title, wt)
                    log(f"Review: {rev}")
                    update_memory(title, rev, wt)
                    if merge_worktree(title, wt):
                        set_item_status(title, "x")
                        run("git add BACKLOG.md .monitor/ && git commit -m f'chore(monitor): complete {title}' || true")
                    del active[title]

            # Spawn new loops if slots available
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

                    prompt = generate_prompt(title, item["body"])
                    if not prompt:
                        log(f"Failed to generate prompt for {title}, skipping")
                        set_item_status(title, " ")
                        run(f"git worktree remove {wt} --force || true")
                        continue

                    (wt / "PROMPT.md").write_text(prompt)
                    # Copy AGENTS.md into worktree
                    agents = AP_DIR / "AGENTS.md"
                    if agents.exists():
                        (wt / "AGENTS.md").write_text(agents.read_text())

                    run(f"git add PROMPT.md AGENTS.md && git commit -m 'chore: init {title}' || true", cwd=wt)
                    run(f"git add BACKLOG.md && git commit -m 'chore(monitor): start {title}' || true")

                    if spawn_ralph(wt):
                        active[title] = wt
                    else:
                        set_item_status(title, " ")

            if not active:
                pending = [i for i in get_backlog_items() if i["status"] == " "]
                if not pending:
                    log("🎉 All backlog items complete!")
                    break

        except Exception as e:
            log(f"Monitor error: {e}")
            import traceback
            log(traceback.format_exc())

        time.sleep(CHECK_INTERVAL)


if __name__ == "__main__":
    main()
