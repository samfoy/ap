#!/usr/bin/env python3
"""
ap-monitor.py — Persistent monitor for the ap development loop.

Watches for LOOP_COMPLETE, reviews what was built, picks the next item
from BACKLOG.md, writes a new PROMPT.md, and spawns a fresh Ralph loop.
Runs as a long-lived Pi session with memory of past work.
"""

import json
import subprocess
import time
import re
from pathlib import Path
from datetime import datetime

AP_DIR = Path.home() / "Projects/ap"
BACKLOG = AP_DIR / "BACKLOG.md"
PROMPT = AP_DIR / "PROMPT.md"
MEMORY = AP_DIR / ".monitor/memory.md"
LOG = AP_DIR / ".monitor/monitor.log"
SCRATCHPAD = AP_DIR / ".ralph/agent/scratchpad.md"
CHECK_INTERVAL = 30  # seconds

MEMORY.parent.mkdir(exist_ok=True)
LOG.parent.mkdir(exist_ok=True)


def log(msg):
    ts = datetime.now().strftime("%H:%M:%S")
    line = f"[{ts}] {msg}"
    print(line)
    with open(LOG, "a") as f:
        f.write(line + "\n")


def run(cmd, **kwargs):
    return subprocess.run(cmd, shell=True, capture_output=True, text=True, cwd=AP_DIR, **kwargs)


def loop_is_running():
    """Check if a Ralph loop process is active."""
    r = run("pgrep -f 'ralph run' || true")
    return bool(r.stdout.strip())


def loop_completed():
    """Check if the last loop hit LOOP_COMPLETE."""
    if not SCRATCHPAD.exists():
        return False
    content = SCRATCHPAD.read_text()
    return "LOOP_COMPLETE" in content


def get_next_backlog_item():
    """Return the next uncompleted backlog item title and index."""
    text = BACKLOG.read_text()
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if re.match(r"\d+\. \[ \]", line.strip()):
            # Extract title (between ** **)
            m = re.search(r"\*\*(.+?)\*\*", line)
            title = m.group(1) if m else line.strip()
            # Get full item block (this line + following lines until next numbered item)
            block = [line]
            for j in range(i+1, min(i+5, len(lines))):
                if re.match(r"\d+\. \[", lines[j].strip()):
                    break
                block.append(lines[j])
            return title, "\n".join(block), i
    return None, None, None


def mark_backlog_in_progress(item_index, title):
    text = BACKLOG.read_text()
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if f"**{title}**" in line and "[ ]" in line:
            lines[i] = line.replace("[ ]", "[~]")
            break
    BACKLOG.write_text("\n".join(lines))


def mark_backlog_complete(title):
    text = BACKLOG.read_text()
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if f"**{title}**" in line and ("[~]" in line or "[ ]" in line):
            lines[i] = line.replace("[~]", "[x]").replace("[ ]", "[x]")
            break
    BACKLOG.write_text("\n".join(lines))


def get_completed_summary():
    """Pull last few git commits as a summary of what was just built."""
    r = run("git log --oneline -8")
    return r.stdout.strip()


def update_memory(summary):
    """Append to monitor memory file."""
    with open(MEMORY, "a") as f:
        f.write(f"\n## {datetime.now().strftime('%Y-%m-%d %H:%M')}\n")
        f.write(summary + "\n")


def write_prompt_for_item(title, item_block):
    """Ask Pi to write a detailed PROMPT.md for the next backlog item."""
    context = f"""
You are helping build `ap`, a Rust/ratatui AI coding agent (~/Projects/ap/ap/).
The project uses functional-first Rust: pure functions, immutable data, iterator chains, Middleware chain pattern.

The next development goal is: **{title}**

Backlog item:
{item_block}

Recent completed work (git log):
{get_completed_summary()}

Monitor memory (past context):
{MEMORY.read_text() if MEMORY.exists() else '(none yet)'}

Write a detailed PROMPT.md for a Ralph pdd-to-code-assist loop to implement this goal.
The prompt should include:
- Vision and goals
- Technical requirements (be specific — types, function signatures, config format)
- Implementation plan (ordered steps, each independently compilable)
- Acceptance criteria
- End with: Output LOOP_COMPLETE when all acceptance criteria are met and the project builds clean.

Output only the PROMPT.md content, no preamble.
"""
    r = subprocess.run(
        ["pi", "--provider", "amazon-bedrock", "--model", "us.anthropic.claude-sonnet-4-6", "--print", context],
        capture_output=True, text=True,
        env={**__import__("os").environ}
    )
    if r.returncode == 0 and r.stdout.strip():
        PROMPT.write_text(r.stdout.strip())
        log(f"Wrote new PROMPT.md for: {title}")
        return True
    else:
        log(f"Pi failed to generate prompt: {r.stderr[:200]}")
        return False


def spawn_ralph_loop():
    """Start a new Ralph loop in the background."""
    cmd = "nohup ralph run -H builtin:pdd-to-code-assist --no-tui --continue > .monitor/ralph.log 2>&1 &"
    run(cmd)
    log("Spawned new Ralph loop")
    time.sleep(3)
    r = run("pgrep -f 'ralph run'")
    if r.stdout.strip():
        log(f"Ralph PID: {r.stdout.strip()}")
        return True
    log("WARNING: Ralph may not have started")
    return False


def review_completed_work(title):
    """Quick review via Pi of what was built."""
    commits = get_completed_summary()
    r = subprocess.run(
        ["pi", "--print", f"Review this git log for the ap Rust project. The goal was: {title}\n\nCommits:\n{commits}\n\nIn 2-3 sentences: did it land? Any obvious gaps or concerns?"],
        capture_output=True, text=True,
        env={**__import__("os").environ}
    )
    return r.stdout.strip() if r.returncode == 0 else "(review unavailable)"


def main():
    log("ap monitor started")
    current_item = None

    # Check if FP refactor is already in progress
    text = BACKLOG.read_text()
    if "[~]" in text:
        m = re.search(r"\[~\] \*\*(.+?)\*\*", text)
        if m:
            current_item = m.group(1)
            log(f"Resuming watch on in-progress item: {current_item}")

    while True:
        try:
            running = loop_is_running()
            completed = loop_completed()

            if not running and completed and current_item:
                log(f"Loop completed for: {current_item}")

                # Review
                review = review_completed_work(current_item)
                log(f"Review: {review}")

                # Update memory
                update_memory(f"Completed: {current_item}\nReview: {review}\n{get_completed_summary()}")

                # Mark done
                mark_backlog_complete(current_item)

                # Commit backlog + memory
                run("git add BACKLOG.md .monitor/ && git commit -m 'chore(monitor): mark complete + update memory' || true")

                # Reset scratchpad so next loop is fresh
                if SCRATCHPAD.exists():
                    SCRATCHPAD.write_text("# Scratchpad\n\n")
                    run("git add .ralph/agent/scratchpad.md && git commit -m 'chore(monitor): reset scratchpad for next loop' || true")

                current_item = None

            if not running and not current_item:
                title, item_block, idx = get_next_backlog_item()
                if not title:
                    log("Backlog complete! All items done.")
                    break

                log(f"Next item: {title}")
                mark_backlog_in_progress(idx, title)
                current_item = title

                if write_prompt_for_item(title, item_block):
                    run("git add PROMPT.md BACKLOG.md && git commit -m f'chore(monitor): start {title}' || true")
                    spawn_ralph_loop()
                else:
                    log("Failed to generate prompt, will retry next cycle")
                    # Reset the in-progress marker so we retry same item
                    mark_backlog_complete(title)  # undo [~] → back to [ ]
                    text = BACKLOG.read_text()
                    BACKLOG.write_text(text.replace(f"[x] **{title}**", f"[ ] **{title}**"))
                    current_item = None

        except Exception as e:
            log(f"Monitor error: {e}")

        time.sleep(CHECK_INTERVAL)


if __name__ == "__main__":
    main()
