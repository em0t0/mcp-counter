#![allow(dead_code)]
use std::collections::HashMap;
use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, model::*, schemars, service::RequestContext,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use tokio::sync::Mutex;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct StructRequest {
    pub a: i32,
    pub b: i32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ToolsConfig {
    pub basic_tools: Vec<BasicTool>,
    pub param_tools: Vec<ParamTool>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BasicTool {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ParamTool {
    pub name: String,
    pub description: String,
    pub schema_type: String,
}

type ToolHandler = Arc<
    dyn Fn(
            &Counter,
            Option<JsonValue>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<CallToolResult, McpError>> + Send>,
        > + Send
        + Sync,
>;

#[derive(Clone)]
pub struct Counter {
    counter: Arc<Mutex<i32>>,
    tools: Arc<HashMap<String, ToolHandler>>,
    tool_definitions: Arc<Vec<Tool>>,
}

impl Counter {
    fn load_tools_config() -> Result<ToolsConfig, Box<dyn std::error::Error>> {
        let config_path = "tools.yaml";
        let config_str = std::fs::read_to_string(config_path)?;
        let config: ToolsConfig = serde_yaml::from_str(&config_str)?;
        Ok(config)
    }

    #[allow(dead_code)]
    pub fn new() -> Self {
        let mut tools: HashMap<String, ToolHandler> = HashMap::new();
        let mut tool_definitions = Vec::new();

        // YAMLファイルから設定を読み込み（存在しない場合はツールなし）
        let config = Self::load_tools_config().unwrap_or_else(|e| {
            eprintln!("No tools.yaml found: {}. Starting without tools.", e);
            ToolsConfig {
                basic_tools: vec![],
                param_tools: vec![],
            }
        });

        // 基本ツールを動的に登録
        for tool in &config.basic_tools {
            tools.insert(tool.name.clone(), Self::create_basic_handler(&tool.name));
            tool_definitions.push(Tool {
                name: tool.name.clone().into(),
                title: None,
                description: Some(tool.description.clone().into()),
                input_schema: Arc::new(Self::create_empty_schema()),
                output_schema: None,
                annotations: None,
                icons: None,
            });
        }

        // パラメータ付きツールを登録
        for tool in &config.param_tools {
            let schema = match tool.schema_type.as_str() {
                "echo" => Self::create_echo_schema(),
                "sum" => Self::create_sum_schema(),
                _ => Self::create_empty_schema(),
            };

            tools.insert(tool.name.clone(), Self::create_param_handler(&tool.name));
            tool_definitions.push(Tool {
                name: tool.name.clone().into(),
                title: None,
                description: Some(tool.description.clone().into()),
                input_schema: Arc::new(schema),
                output_schema: None,
                annotations: None,
                icons: None,
            });
        }

        Self {
            counter: Arc::new(Mutex::new(0)),
            tools: Arc::new(tools),
            tool_definitions: Arc::new(tool_definitions),
        }
    }

    fn _create_resource_text(&self, uri: &str, name: &str) -> Resource {
        RawResource::new(uri, name.to_string()).no_annotation()
    }

    fn create_basic_handler(tool_name: &str) -> ToolHandler {
        let name = tool_name.to_string();
        Arc::new(move |counter_instance, _args| {
            let counter = counter_instance.counter.clone();
            let name = name.clone();

            Box::pin(async move {
                match name.as_str() {
                    "increment" => {
                        let mut counter = counter.lock().await;
                        *counter += 1;
                        Ok(CallToolResult::success(vec![Content::text(
                            counter.to_string(),
                        )]))
                    }
                    "decrement" => {
                        let mut counter = counter.lock().await;
                        *counter -= 1;
                        Ok(CallToolResult::success(vec![Content::text(
                            counter.to_string(),
                        )]))
                    }
                    "get_value" => {
                        let counter = counter.lock().await;
                        Ok(CallToolResult::success(vec![Content::text(
                            counter.to_string(),
                        )]))
                    }
                    "reset" => {
                        let mut counter = counter.lock().await;
                        *counter = 0;
                        Ok(CallToolResult::success(vec![Content::text("Reset to 0")]))
                    }
                    "say_hello" => Ok(CallToolResult::success(vec![Content::text("hello")])),
                    _ => Err(McpError::invalid_request("Unknown tool", None)),
                }
            })
        })
    }

    fn create_param_handler(tool_name: &str) -> ToolHandler {
        let name = tool_name.to_string();
        Arc::new(move |_counter_instance, args| {
            let name = name.clone();
            Box::pin(async move {
                match name.as_str() {
                    "echo" => {
                        let message = if let Some(args) = &args {
                            args.get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("No message provided")
                        } else {
                            "No message provided"
                        };
                        Ok(CallToolResult::success(vec![Content::text(format!(
                            "Echo: {}",
                            message
                        ))]))
                    }
                    "sum" => {
                        let (a, b) = if let Some(args) = &args {
                            let a = args.get("a").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                            let b = args.get("b").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                            (a, b)
                        } else {
                            (0, 0)
                        };
                        Ok(CallToolResult::success(vec![Content::text(
                            (a + b).to_string(),
                        )]))
                    }
                    _ => Err(McpError::invalid_request("Unknown tool", None)),
                }
            })
        })
    }

    fn create_empty_schema() -> serde_json::Map<String, JsonValue> {
        let mut schema = serde_json::Map::new();
        schema.insert("type".to_string(), json!("object"));
        schema.insert("properties".to_string(), json!({}));
        schema
    }

    fn create_echo_schema() -> serde_json::Map<String, JsonValue> {
        let mut schema = serde_json::Map::new();
        schema.insert("type".to_string(), json!("object"));
        let mut properties = serde_json::Map::new();
        properties.insert(
            "message".to_string(),
            json!({
                "type": "string",
                "description": "Message to echo back"
            }),
        );
        schema.insert("properties".to_string(), JsonValue::Object(properties));
        schema.insert("required".to_string(), json!(["message"]));
        schema
    }

    fn create_sum_schema() -> serde_json::Map<String, JsonValue> {
        let mut schema = serde_json::Map::new();
        schema.insert("type".to_string(), json!("object"));
        let mut properties = serde_json::Map::new();
        properties.insert(
            "a".to_string(),
            json!({
                "type": "integer",
                "description": "First number"
            }),
        );
        properties.insert(
            "b".to_string(),
            json!({
                "type": "integer",
                "description": "Second number"
            }),
        );
        schema.insert("properties".to_string(), JsonValue::Object(properties));
        schema.insert("required".to_string(), json!(["a", "b"]));
        schema
    }
}
impl ServerHandler for Counter {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_prompts()
                .enable_resources()
                .enable_tools()
                .build(),
            server_info: Implementation::from_build_env(),
            instructions: Some("Dynamic tool server with counter functionality. Tools are registered dynamically at runtime using for loops.".to_string()),
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: self.tool_definitions.as_ref().clone(),
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        CallToolRequestParam { name, arguments }: CallToolRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if let Some(handler) = self.tools.get(name.as_ref()) {
            let args = arguments.map(|args| JsonValue::Object(args));
            handler(self, args).await
        } else {
            Err(McpError::invalid_request("Tool not found", None))
        }
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: vec![
                self._create_resource_text("str:////Users/to/some/path/", "cwd"),
                self._create_resource_text("memo://insights", "memo-name"),
            ],
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        ReadResourceRequestParam { uri }: ReadResourceRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        match uri.as_str() {
            "str:////Users/to/some/path/" => {
                let cwd = "/Users/to/some/path/";
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(cwd, uri)],
                })
            }
            "memo://insights" => {
                let memo = "Business Intelligence Memo\n\nAnalysis has revealed 5 key insights ...";
                Ok(ReadResourceResult {
                    contents: vec![ResourceContents::text(memo, uri)],
                })
            }
            _ => Err(McpError::resource_not_found(
                "resource_not_found",
                Some(json!({
                    "uri": uri
                })),
            )),
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult {
            next_cursor: None,
            prompts: vec![Prompt::new(
                "example_prompt",
                Some("This is an example prompt that takes one required argument, message"),
                Some(vec![PromptArgument {
                    name: "message".to_string(),
                    title: Some("Message".to_string()),
                    description: Some("A message to put in the prompt".to_string()),
                    required: Some(true),
                }]),
            )],
        })
    }

    async fn get_prompt(
        &self,
        GetPromptRequestParam { name, arguments }: GetPromptRequestParam,
        _: RequestContext<RoleServer>,
    ) -> Result<GetPromptResult, McpError> {
        match name.as_str() {
            "example_prompt" => {
                let message = arguments
                    .and_then(|json| json.get("message")?.as_str().map(|s| s.to_string()))
                    .ok_or_else(|| {
                        McpError::invalid_params("No message provided to example_prompt", None)
                    })?;

                let prompt =
                    format!("This is an example prompt with your message here: '{message}'");
                Ok(GetPromptResult {
                    description: None,
                    messages: vec![PromptMessage {
                        role: PromptMessageRole::User,
                        content: PromptMessageContent::text(prompt),
                    }],
                })
            }
            _ => Err(McpError::invalid_params("prompt not found", None)),
        }
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParam>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult {
            next_cursor: None,
            resource_templates: Vec::new(),
        })
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, McpError> {
        if let Some(http_request_part) = context.extensions.get::<axum::http::request::Parts>() {
            let initialize_headers = &http_request_part.headers;
            let initialize_uri = &http_request_part.uri;
            tracing::info!(?initialize_headers, %initialize_uri, "initialize from http server");
        }
        Ok(self.get_info())
    }
}
