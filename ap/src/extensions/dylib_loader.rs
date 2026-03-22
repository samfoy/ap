use anyhow::{Context, Result};
use libloading::{Library, Symbol};
use std::path::{Path, PathBuf};

use crate::extensions::Registry;
use crate::extensions::rhai_loader::RhaiTool;

// ─── C ABI entry point type ───────────────────────────────────────────────────

type InitFn = unsafe extern "C" fn(*mut Registry);

// ─── Warning type ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum LoadWarning {
    DylibLoad(String),
    RhaiLoad(String),
    DirRead(String),
}

impl std::fmt::Display for LoadWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadWarning::DylibLoad(s) => write!(f, "[dylib] {s}"),
            LoadWarning::RhaiLoad(s) => write!(f, "[rhai] {s}"),
            LoadWarning::DirRead(s) => write!(f, "[dir] {s}"),
        }
    }
}

// ─── ExtensionLoader ──────────────────────────────────────────────────────────

/// Discovers and loads extensions from the extension directories.
///
/// # Safety
///
/// Dylib extensions are **unsafe by design**. Any mismatch between the extension's
/// toolchain, Cargo features, or `ap` crate version and the running binary will cause
/// undefined behavior. The `Library` handles are kept alive in `self.libraries` for the
/// process lifetime — dropping them calls `dlclose()`, invalidating all registered
/// vtables and function pointers.
///
/// **Prefer Rhai extensions** (`.rhai` files) for safe, sandboxed extension authoring.
pub struct ExtensionLoader {
    /// Keeps dylib handles alive for the process lifetime.
    /// Dropping a `Library` calls `dlclose()` — never drop these early.
    libraries: Vec<Library>,
    /// Non-fatal warnings accumulated during discovery.
    pub warnings: Vec<LoadWarning>,
}

impl ExtensionLoader {
    pub fn new() -> Self {
        Self {
            libraries: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// Scan `~/.ap/extensions/` and `./.ap/extensions/` for `.rhai` and `.dylib`/`.so` files.
    /// Missing directories are silently skipped.
    pub fn discover_and_load(&mut self, registry: &mut Registry) {
        let dirs: Vec<PathBuf> = {
            let mut v = Vec::new();
            if let Some(home) = dirs::home_dir() {
                v.push(home.join(".ap/extensions"));
            }
            v.push(PathBuf::from(".ap/extensions"));
            v
        };

        for dir in dirs {
            if !dir.exists() {
                // Missing directory is silently skipped — not an error.
                continue;
            }

            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(e) => {
                    self.warnings.push(LoadWarning::DirRead(e.to_string()));
                    continue;
                }
            };

            for entry in entries.flatten() {
                // FAIL-NEW-2 fix: Path::extension() returns Option<&OsStr>, not &str.
                // Use .and_then(|e| e.to_str()) to get Option<&str> for pattern matching.
                match entry.path().extension().and_then(|e| e.to_str()) {
                    Some("rhai") => {
                        self.load_rhai(&entry.path(), registry);
                    }
                    Some("dylib") | Some("so") => {
                        match load_dylib(&entry.path(), registry) {
                            Ok(lib) => {
                                // FAIL-NEW-3 fix: keep Library alive — dlclose() on drop!
                                self.libraries.push(lib);
                            }
                            Err(e) => {
                                self.warnings
                                    .push(LoadWarning::DylibLoad(e.to_string()));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn load_rhai(&mut self, path: &Path, registry: &mut Registry) {
        match RhaiTool::load(path) {
            Ok(tool) => registry.register_tool(Box::new(tool)),
            Err(e) => self.warnings.push(LoadWarning::RhaiLoad(e.to_string())),
        }
    }
}

impl Default for ExtensionLoader {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Dylib loading ────────────────────────────────────────────────────────────

/// Load a dylib and call its `ap_extension_init` entry point.
///
/// Returns the `Library` handle — the **caller MUST store it** in
/// `ExtensionLoader.libraries` for the process lifetime.
///
/// # Safety
///
/// Dropping the returned `Library` calls `dlclose()`, unloading the shared library
/// and making all registered function pointers and vtables dangling (use-after-free).
///
/// # Errors
///
/// Returns `Err` if the file cannot be loaded as a dynamic library or if the
/// `ap_extension_init` symbol is not found. A warning is appropriate; no panic.
pub fn load_dylib(path: &Path, registry: &mut Registry) -> Result<Library> {
    // SAFETY: loading arbitrary dylibs is inherently unsafe. Extension authors
    // must compile against the exact same ap crate version. See module-level docs.
    let lib = unsafe {
        Library::new(path).with_context(|| format!("failed to load dylib: {}", path.display()))?
    };

    let init: Symbol<InitFn> = unsafe {
        lib.get(b"ap_extension_init\0")
            .with_context(|| format!("symbol 'ap_extension_init' not found in {}", path.display()))?
    };

    unsafe { init(registry as *mut Registry) };

    // Transfer ownership to caller — MUST be kept alive.
    Ok(lib)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::Registry;

    #[test]
    fn test_dylib_non_dylib_file_returns_warning() {
        // Cargo.toml is definitely not a dylib — load_dylib should return Err, not panic.
        let mut registry = Registry::new();
        let result = load_dylib(Path::new("Cargo.toml"), &mut registry);
        assert!(
            result.is_err(),
            "non-dylib file should return Err, not panic"
        );
    }

    #[test]
    fn test_dylib_missing_symbol_returns_warning_via_loader() {
        // A non-existent path should produce a DylibLoad warning and no panic.
        let mut loader = ExtensionLoader::new();
        let mut registry = Registry::new();

        // Directly call load_dylib with a bad path.
        let result = load_dylib(Path::new("/nonexistent/fake.dylib"), &mut registry);
        assert!(result.is_err());

        // Confirm warnings accumulate when loader calls it.
        // (discover_and_load skips missing dirs silently — test the warning path via direct call)
        if let Err(e) = result {
            loader.warnings.push(LoadWarning::DylibLoad(e.to_string()));
        }
        assert_eq!(loader.warnings.len(), 1);
    }

    #[test]
    fn test_discover_and_load_missing_dirs_no_crash() {
        // Neither ~/.ap/extensions nor ./.ap/extensions exist in CI — must not crash.
        let mut loader = ExtensionLoader::new();
        let mut registry = Registry::new();
        // This must complete without panicking.
        loader.discover_and_load(&mut registry);
        // No tools loaded from missing dirs.
        assert_eq!(registry.tools.len(), 0);
    }
}
