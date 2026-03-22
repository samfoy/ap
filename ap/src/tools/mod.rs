use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

pub mod bash;
pub mod edit;
pub mod read;
pub mod write;

pub use bash::BashTool;
pub use edit::EditTool;
pub use read::ReadTool;
pub use write::WriteTool;

/// The result returned by any tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}

impl ToolResult {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    pub fn err(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

/// Object-safe async tool trait.
///
/// Uses `BoxFuture` so tools can be stored as `Box<dyn Tool>`.
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// Returns a JSON Schema object describing the tool's parameters.
    fn schema(&self) -> serde_json::Value;
    /// Execute the tool with the given parameters.
    fn execute(&self, params: serde_json::Value) -> BoxFuture<'_, ToolResult>;
}

/// Registry of available tools.
pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Create a registry pre-populated with the 4 built-in tools.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(ReadTool));
        registry.register(Box::new(WriteTool));
        registry.register(Box::new(EditTool));
        registry.register(Box::new(BashTool));
        registry
    }

    /// Register a tool.
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }

    /// Find a tool by name.
    pub fn find_by_name(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
    }

    /// Consuming builder — register a tool and return `self` (chainable).
    ///
    /// ```rust,ignore
    /// let registry = ToolRegistry::new().with(ReadTool).with(WriteTool);
    /// ```
    pub fn with(mut self, tool: impl Tool + 'static) -> Self {
        self.tools.push(Box::new(tool));
        self
    }

    /// Return all tool schemas (used to inject into Bedrock API calls).
    pub fn all_schemas(&self) -> Vec<serde_json::Value> {
        self.tools.iter().map(|t| t.schema()).collect()
    }

    /// Alias for `all_schemas()` — returns the JSON schema for every registered tool.
    pub fn tool_schemas(&self) -> Vec<serde_json::Value> {
        self.all_schemas()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_with_defaults_has_four_schemas() {
        let registry = ToolRegistry::with_defaults();
        let schemas = registry.all_schemas();
        assert_eq!(schemas.len(), 4);
        // Each schema must have a "name" field
        for schema in &schemas {
            assert!(schema.get("name").is_some(), "schema missing 'name': {:?}", schema);
        }
    }

    #[test]
    fn test_registry_find_by_name() {
        let registry = ToolRegistry::with_defaults();
        assert!(registry.find_by_name("read").is_some());
        assert!(registry.find_by_name("write").is_some());
        assert!(registry.find_by_name("edit").is_some());
        assert!(registry.find_by_name("bash").is_some());
        assert!(registry.find_by_name("nonexistent").is_none());
    }

    // AC5: ToolRegistry .with() builder chains
    #[test]
    fn registry_with_builder_chains_tools() {
        let registry = ToolRegistry::new().with(ReadTool).with(WriteTool);
        assert_eq!(registry.tool_schemas().len(), 2);
    }

    // AC5 (consuming): .with() returns Self — verified by chaining
    #[test]
    fn registry_with_builder_is_consuming() {
        let r = ToolRegistry::new().with(ReadTool).with(WriteTool).with(EditTool);
        assert_eq!(r.tool_schemas().len(), 3);
    }

    // AC6: with_defaults() still registers 4 tools
    #[test]
    fn registry_with_defaults_still_works() {
        let registry = ToolRegistry::with_defaults();
        assert_eq!(registry.tool_schemas().len(), 4);
    }
}
