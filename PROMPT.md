# PROMPT.md — Project-level Config (Backlog item 22)

## Vision

`ap` currently loads config from `~/.ap/config.toml` (global) and hardcodes
`./ap.toml` as the project config path. This is wrong in two ways: the
project config should live at `.ap/config.toml` (consistent with ap's other
project-local directories), and it should be discovered by walking up from
`cwd` to the git root rather than only checking the current directory.

After this feature:
- `~/.ap/config.toml` — global defaults
- `.ap/config.toml` — project override, auto-discovered from cwd upward to
  git root (first match wins)
- CLI flags (`--model`, `--region`, `--no-project-config`) — highest priority
- Priority order: defaults → global → project → CLI

The implementation is contained entirely within `src/config.rs` and
`src/main.rs`. All existing tests continue to pass; new tests cover every new
code path.

---

## Codebase orientation

```
ap/src/
  config.rs        ← all config loading logic; add discovery here
  main.rs          ← wires AppConfig::load(); add CLI flags here
```

Key existing API (do not break):
```rust
// Testable entry point — keep signature unchanged
pub fn load_with_paths(
    global_path: Option<&Path>,
    project_path: Option<&Path>,
) -> Result<AppConfig>

// Public load called from main — signature WILL change (see Step 2)
pub fn load() -> Result<AppConfig>
```

All 203 existing tests must remain green throughout.

---

## Technical requirements

### 1. `discover_project_config(start: &Path) -> Option<PathBuf>`

Pure function in `src/config.rs`. Walks from `start` up the filesystem,
stopping at the first directory that is a git root (contains `.git/`) or at
the filesystem root (whichever comes first). Returns the first
`.ap/config.toml` found along that walk, or `None`.

```rust
/// Walk from `start` toward the git root (directory containing `.git/`),
/// checking each directory for `.ap/config.toml`.
/// Returns the path of the first match, or `None` if none found.
pub fn discover_project_config(start: &Path) -> Option<PathBuf>
```

Behaviour contract:
- Checks `start/.ap/config.toml` first, then `start/../.ap/config.toml`, etc.
- Stops **after** checking the git-root directory (i.e. the directory
  containing `.git/` is checked before stopping, it is not skipped).
- If no `.git/` is ever found, walks all the way to the filesystem root.
- Returns `None` rather than erroring if the file does not exist.
- Never panics; never does I/O other than existence checks.

### 2. `AppConfig::load(skip_project_config: bool) -> Result<AppConfig>`

Replace the current `load()` (which takes no arguments) with one that accepts
a bool flag:

```rust
pub fn load(skip_project_config: bool) -> Result<AppConfig>
```

Implementation:
```rust
pub fn load(skip_project_config: bool) -> Result<AppConfig> {
    let global = dirs::home_dir().map(|h| h.join(".ap").join("config.toml"));

    let project = if skip_project_config {
        None
    } else {
        std::env::current_dir()
            .ok()
            .and_then(|cwd| discover_project_config(&cwd))
    };

    Self::load_with_paths(global.as_deref(), project.as_deref())
}
```

The existing `load_with_paths` is unchanged.

### 3. CLI flags in `src/main.rs`

Add three new arguments to the `Args` struct:

```rust
/// Skip project-level .ap/config.toml discovery
#[arg(long, default_value_t = false)]
no_project_config: bool,

/// Override the provider model from the config file
#[arg(long)]
model: Option<String>,

/// Override the AWS region from the config file
#[arg(long)]
region: Option<String>,
```

Wire them in `main()` after `AppConfig::load(…)`:

```rust
let mut config = AppConfig::load(args.no_project_config).unwrap_or_default();

if let Some(limit) = args.context_limit { config.context.limit = Some(limit); }
if let Some(model) = args.model        { config.provider.model = model; }
if let Some(region) = args.region      { config.provider.region = region; }
```

---

## Ordered implementation steps

Each step must leave the project in a **compilable, all-tests-green** state
before moving to the next.

### Step 1 — `discover_project_config` + unit tests

In `src/config.rs`, add the function and a `#[cfg(test)]` module section
(`mod discovery_tests`) with the following cases:

| Test name | Scenario |
|---|---|
| `discover_finds_file_in_cwd` | `.ap/config.toml` exists in `start` → returns it |
| `discover_finds_file_in_parent` | file only in parent of `start` → returns parent path |
| `discover_stops_at_git_root` | `.git/` in grandparent, file only in great-grandparent → returns `None` |
| `discover_git_root_itself_checked` | `.git/` and `.ap/config.toml` in same dir → returns it |
| `discover_returns_none_when_absent` | no file anywhere → returns `None` |
| `discover_walks_to_fs_root_when_no_git` | no `.git/` found, file not present → returns `None` (no panic) |

Use `tempfile::TempDir` to build real directory trees for each test.

**Do not touch `load()` or `main.rs` in this step.**

### Step 2 — Update `AppConfig::load()` signature

Change the signature from `load() -> Result<AppConfig>` to
`load(skip_project_config: bool) -> Result<AppConfig>` and update the body
to use `discover_project_config` as shown in the requirements above.

Add unit tests in the existing `mod tests` block:

| Test name | Scenario |
|---|---|
| `load_skip_project_config_ignores_project_file` | With `skip_project_config = true`, a project file in cwd is NOT applied |
| `load_uses_discovered_project_config` | With `skip_project_config = false` and a `.ap/config.toml` in cwd, it IS applied |

For `load_uses_discovered_project_config`, temporarily change the working
directory using `std::env::set_current_dir` inside the test (restore it
after), or use `load_with_paths` directly against a known temp path — whichever
avoids flaky behaviour in parallel test runs. Prefer `load_with_paths` for
isolation.

**Do not touch `main.rs` in this step.**

### Step 3 — CLI flags in `main.rs`

Update `src/main.rs`:
1. Add `no_project_config`, `model`, `region` to `Args`.
2. Change `AppConfig::load()` call to `AppConfig::load(args.no_project_config)`.
3. Apply `model` and `region` CLI overrides after the load.
4. Extend the existing `#[cfg(test)] mod tests` with:

| Test name | Scenario |
|---|---|
| `args_no_project_config_default_false` | Confirm `no_project_config` defaults to `false` via clap |

Use `clap`'s `try_parse_from` for CLI tests.

### Step 4 — Integration smoke test

Add a new integration test in `tests/` named `project_config.rs`:

```rust
// tests/project_config.rs
//
// Smoke-tests that project-level .ap/config.toml is discovered
// and applied ahead of the global config.
```

Test: spin up a temp dir with a `.ap/config.toml` that sets a non-default
model, call `AppConfig::load_with_paths(None, Some(&path))`, assert the model
is applied and non-overridden defaults are preserved.

This test must not require network access, AWS credentials, or a real git repo.

---

## Acceptance criteria

All of the following must be true before outputting `LOOP_COMPLETE`:

- [ ] **AC1** `cargo build` completes with zero errors and zero warnings on
  `src/config.rs` and `src/main.rs`.
- [ ] **AC2** `cargo test` passes all 203 existing tests plus every new test
  introduced in Steps 1–4.
- [ ] **AC3** `discover_project_config` is a pure function (no `mut` state, no
  I/O beyond `Path::exists` / `Path::is_dir`, no `unwrap`/`expect` outside
  `#[cfg(test)]`).
- [ ] **AC4** `AppConfig::load(skip_project_config: bool)` signature is in
  place; the old zero-argument `load()` no longer exists.
- [ ] **AC5** `--no-project-config`, `--model`, `--region` flags are present
  in `ap --help` output.
- [ ] **AC6** Discovery walks up to (and including) the git root but no
  further; confirmed by `discover_stops_at_git_root` test.
- [ ] **AC7** Project config path is `.ap/config.toml`, not `ap.toml`; the
  string `"ap.toml"` no longer appears as a literal in `config.rs`.
- [ ] **AC8** All `clippy::unwrap_used` and `clippy::expect_used` lints
  remain satisfied in non-test code.
- [ ] **AC9** No new `mut` bindings outside test helpers; the functional-first
  style is preserved.
- [ ] **AC10** `cargo clippy -- -D warnings` exits 0.

---

Output `LOOP_COMPLETE` when all acceptance criteria are met and the project
builds clean.
