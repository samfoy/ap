use crate::provider::Message;
use crate::tools::Tool;

pub mod dylib_loader;
pub mod rhai_loader;

pub use dylib_loader::ExtensionLoader;
pub use rhai_loader::RhaiTool;

// ─── Hook registration ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum HookLifecycle {
    PreToolCall,
    PostToolCall,
    PreTurn,
    PostTurn,
    OnError,
}

#[derive(Debug, Clone)]
pub struct HookRegistration {
    pub lifecycle: HookLifecycle,
    pub command: String,
}

// ─── Stub traits ──────────────────────────────────────────────────────────────

/// v1 stub — collected but not rendered.
pub trait Panel: Send + Sync {
    fn name(&self) -> &str;
    // Rendering methods added in v2
}

/// v1 stub — collected but not invoked.
pub trait MessageInterceptor: Send + Sync {
    fn name(&self) -> &str;
    fn intercept(&self, msg: &Message) -> Message;
}

// ─── Extension trait ──────────────────────────────────────────────────────────

pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn register(&self, registry: &mut Registry);
}

// ─── Registry ─────────────────────────────────────────────────────────────────

/// Collects all extension-provided surfaces.
///
/// - `tools`: live in v1 — registered tools are available to the agent loop.
/// - `hooks`: collected in v1, not invoked (v2 wires execution).
/// - `panels`: collected in v1, not rendered (v2 wires rendering).
/// - `message_interceptors`: collected in v1, not invoked (v2 wires interception).
pub struct Registry {
    pub tools: Vec<Box<dyn Tool>>,
    pub hooks: Vec<HookRegistration>,
    pub panels: Vec<Box<dyn Panel>>,
    pub message_interceptors: Vec<Box<dyn MessageInterceptor>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            tools: Vec::new(),
            hooks: Vec::new(),
            panels: Vec::new(),
            message_interceptors: Vec::new(),
        }
    }

    pub fn register_tool(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }

    pub fn register_hook(&mut self, lifecycle: HookLifecycle, command: String) {
        self.hooks.push(HookRegistration { lifecycle, command });
    }

    pub fn register_panel(&mut self, panel: Box<dyn Panel>) {
        self.panels.push(panel);
    }

    pub fn register_message_interceptor(&mut self, interceptor: Box<dyn MessageInterceptor>) {
        self.message_interceptors.push(interceptor);
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}
