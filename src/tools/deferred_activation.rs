use super::traits::{Tool, ToolResult, ToolSpec};
use async_trait::async_trait;
use serde_json::json;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredToolStub {
    pub name: String,
    pub description: String,
}

#[derive(Clone)]
struct DeferredToolEntry {
    stub: DeferredToolStub,
    spec: ToolSpec,
    tool: Arc<dyn Tool>,
}

impl DeferredToolEntry {
    fn new(tool: Arc<dyn Tool>) -> Self {
        let spec = tool.spec();
        let stub = DeferredToolStub {
            name: spec.name.clone(),
            description: spec.description.clone(),
        };

        Self { stub, spec, tool }
    }

    fn score(&self, query: &str) -> usize {
        let normalized = query.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return 0;
        }

        let tool_name = self.spec.name.to_ascii_lowercase();
        let description = self.spec.description.to_ascii_lowercase();
        let schema = self.spec.parameters.to_string().to_ascii_lowercase();

        if tool_name == normalized {
            return 240;
        }
        if tool_name.contains(&normalized) {
            return 160;
        }

        let mut score = 0usize;
        for token in normalized.split_whitespace() {
            if token.is_empty() {
                continue;
            }
            if tool_name.contains(token) {
                score += 48;
            }
            if description.contains(token) {
                score += 24;
            }
            if schema.contains(token) {
                score += 8;
            }
        }

        score
    }
}

#[derive(Clone, Default)]
pub struct ActivatedToolSet {
    tools: Arc<Mutex<HashMap<String, Arc<dyn Tool>>>>,
}

impl ActivatedToolSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self, name: &str) -> bool {
        self.tools
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains_key(name)
    }

    fn activate_entries(&self, entries: &[Arc<DeferredToolEntry>]) -> Vec<String> {
        let mut active = self.tools.lock().unwrap_or_else(|error| error.into_inner());
        let mut activated = Vec::new();

        for entry in entries {
            if active
                .insert(entry.spec.name.clone(), Arc::clone(&entry.tool))
                .is_none()
            {
                activated.push(entry.spec.name.clone());
            }
        }

        activated.sort();
        activated
    }

    pub fn active_tools(&self) -> Vec<Arc<dyn Tool>> {
        let active = self.tools.lock().unwrap_or_else(|error| error.into_inner());
        let mut tools: Vec<(String, Arc<dyn Tool>)> = active
            .iter()
            .map(|(name, tool)| (name.clone(), Arc::clone(tool)))
            .collect();
        tools.sort_by(|left, right| left.0.cmp(&right.0));
        tools.into_iter().map(|(_, tool)| tool).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredToolMatch {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub score: usize,
    pub activated: bool,
}

#[derive(Clone, Default)]
pub struct DeferredToolCatalog {
    entries: Arc<Vec<Arc<DeferredToolEntry>>>,
}

impl DeferredToolCatalog {
    pub fn new(tools: Vec<Arc<dyn Tool>>) -> Self {
        let mut entries: Vec<Arc<DeferredToolEntry>> = tools
            .into_iter()
            .map(DeferredToolEntry::new)
            .map(Arc::new)
            .collect();
        entries.sort_by(|left, right| left.stub.name.cmp(&right.stub.name));
        Self {
            entries: Arc::new(entries),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains_name(&self, name: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.spec.name.eq_ignore_ascii_case(name))
    }

    pub fn visible_stubs(&self, activated: &ActivatedToolSet) -> Vec<DeferredToolStub> {
        self.entries
            .iter()
            .filter(|entry| !activated.is_active(&entry.spec.name))
            .map(|entry| entry.stub.clone())
            .collect()
    }

    fn parse_select_names<'a>(&self, query: &'a str) -> Option<Vec<&'a str>> {
        let trimmed = query.trim();
        let select = trimmed.strip_prefix("select:")?;
        let names: Vec<&str> = select
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .collect();
        (!names.is_empty()).then_some(names)
    }

    fn keyword_matches(&self, query: &str, limit: usize) -> Vec<Arc<DeferredToolEntry>> {
        let mut ranked: Vec<(usize, Arc<DeferredToolEntry>)> = self
            .entries
            .iter()
            .map(|entry| (entry.score(query), Arc::clone(entry)))
            .filter(|(score, _)| *score > 0)
            .collect();
        ranked.sort_by_key(|(score, entry)| (Reverse(*score), entry.spec.name.clone()));
        ranked.truncate(limit);
        ranked.into_iter().map(|(_, entry)| entry).collect()
    }

    fn exact_matches(&self, names: &[&str]) -> (Vec<Arc<DeferredToolEntry>>, Vec<String>) {
        let mut matches = Vec::new();
        let mut missing = Vec::new();

        for requested_name in names {
            if let Some(entry) = self
                .entries
                .iter()
                .find(|entry| entry.spec.name.eq_ignore_ascii_case(requested_name))
            {
                matches.push(Arc::clone(entry));
            } else {
                missing.push((*requested_name).to_string());
            }
        }

        (matches, missing)
    }

    fn to_matches(
        &self,
        entries: &[Arc<DeferredToolEntry>],
        activated: &ActivatedToolSet,
        score_query: Option<&str>,
    ) -> Vec<DeferredToolMatch> {
        entries
            .iter()
            .map(|entry| DeferredToolMatch {
                name: entry.spec.name.clone(),
                description: entry.spec.description.clone(),
                parameters: entry.spec.parameters.clone(),
                score: score_query.map_or(0, |query| entry.score(query)),
                activated: activated.is_active(&entry.spec.name),
            })
            .collect()
    }
}

#[derive(Clone)]
pub struct DeferredToolContext {
    catalog: DeferredToolCatalog,
    activated: ActivatedToolSet,
    tool_search: Arc<ToolSearchTool>,
}

impl DeferredToolContext {
    pub fn new(catalog: DeferredToolCatalog) -> Self {
        let activated = ActivatedToolSet::new();
        let tool_search = Arc::new(ToolSearchTool::new(catalog.clone(), activated.clone()));
        Self {
            catalog,
            activated,
            tool_search,
        }
    }

    pub fn from_tools(tools: Vec<Arc<dyn Tool>>) -> Self {
        Self::new(DeferredToolCatalog::new(tools))
    }

    pub fn has_deferred_tools(&self) -> bool {
        !self.catalog.is_empty()
    }

    pub fn extra_tools(&self) -> Vec<Arc<dyn Tool>> {
        let mut extra_tools = self.activated.active_tools();
        if !self.catalog.is_empty() {
            let tool_search: Arc<dyn Tool> = self.tool_search.clone();
            extra_tools.insert(0, tool_search);
        }
        extra_tools
    }

    pub fn deferred_stubs(&self) -> Vec<DeferredToolStub> {
        self.catalog.visible_stubs(&self.activated)
    }

    pub fn is_deferred_tool(&self, name: &str) -> bool {
        self.catalog.contains_name(name) && !self.activated.is_active(name)
    }
}

pub struct ToolSearchTool {
    catalog: DeferredToolCatalog,
    activated: ActivatedToolSet,
}

impl ToolSearchTool {
    pub fn new(catalog: DeferredToolCatalog, activated: ActivatedToolSet) -> Self {
        Self { catalog, activated }
    }
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "tool_search"
    }

    fn description(&self) -> &str {
        "Search deferred MCP-style tools. Use a keyword query to inspect matches, or `select:name1,name2` to activate exact tools and fetch full schemas."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Keyword query, or `select:name1,name2` to activate exact deferred tools."
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of keyword matches to return.",
                    "minimum": 1,
                    "maximum": 20
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args
            .get("query")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("tool_search requires a non-empty 'query' string"))?;
        let limit = args
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map(|value| value.clamp(1, 20) as usize)
            .unwrap_or(5);

        let (matches, activated, missing, next_step) = if let Some(requested_names) =
            self.catalog.parse_select_names(query)
        {
            let (entries, missing) = self.catalog.exact_matches(&requested_names);
            let activated = self.activated.activate_entries(&entries);
            let matches = self.catalog.to_matches(&entries, &self.activated, None);
            (
                matches,
                activated,
                missing,
                "Call an activated tool directly by name in the next tool call.".to_string(),
            )
        } else {
            let entries = self.catalog.keyword_matches(query, limit);
            let matches = self
                .catalog
                .to_matches(&entries, &self.activated, Some(query));
            (
                    matches,
                    Vec::new(),
                    Vec::new(),
                    "If a listed tool is needed, call tool_search again with `select:<tool_name>` to activate it.".to_string(),
                )
        };

        Ok(ToolResult {
            success: true,
            output: serde_json::to_string_pretty(&json!({
                "query": query,
                "matches": matches
                    .iter()
                    .map(|tool| json!({
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                        "score": tool.score,
                        "activated": tool.activated,
                    }))
                    .collect::<Vec<_>>(),
                "activated": activated,
                "missing": missing,
                "next_step": next_step,
            }))?,
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyTool {
        name: &'static str,
        description: &'static str,
    }

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            self.name
        }

        fn description(&self) -> &str {
            self.description
        }

        fn parameters_schema(&self) -> serde_json::Value {
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                }
            })
        }

        async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                success: true,
                output: format!("executed {}", self.name),
                error: None,
            })
        }
    }

    #[tokio::test]
    async fn tool_search_keyword_query_returns_match_without_activation() {
        let context = DeferredToolContext::from_tools(vec![Arc::new(DummyTool {
            name: "camera_snap",
            description: "Capture a photo from the connected device camera",
        })]);

        let result = context
            .tool_search
            .execute(json!({"query": "camera photo"}))
            .await
            .expect("tool_search should succeed");

        assert!(result.success);
        assert!(result.output.contains("\"camera_snap\""));
        assert!(result.output.contains("\"activated\": false"));
        assert_eq!(context.deferred_stubs().len(), 1);
    }

    #[tokio::test]
    async fn tool_search_select_activates_exact_match() {
        let context = DeferredToolContext::from_tools(vec![Arc::new(DummyTool {
            name: "browser_open",
            description: "Open a browser window",
        })]);

        let result = context
            .tool_search
            .execute(json!({"query": "select:browser_open"}))
            .await
            .expect("tool_search should succeed");

        assert!(result.success);
        assert!(result.output.contains("\"browser_open\""));
        assert!(context
            .extra_tools()
            .iter()
            .any(|tool| tool.name() == "browser_open"));
        assert!(context.deferred_stubs().is_empty());
    }

    #[test]
    fn deferred_tool_context_marks_only_inactive_entries_as_stubs() {
        let context = DeferredToolContext::from_tools(vec![
            Arc::new(DummyTool {
                name: "camera_snap",
                description: "Capture a photo",
            }),
            Arc::new(DummyTool {
                name: "notifications_list",
                description: "List notifications",
            }),
        ]);

        assert!(context.is_deferred_tool("camera_snap"));
        assert_eq!(context.deferred_stubs().len(), 2);
    }
}
