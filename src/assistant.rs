use crate::tools::{Tool, Toolchain};
use reqwest;
use serde_json::{Value, json};
use std::collections::HashMap;

pub type ProgressCallback = Box<dyn Fn(&str) + Send + Sync>;

#[derive(Debug)]
pub enum AssistantError {
    RequestError(reqwest::Error),
    JsonError(serde_json::Error),
    ToolError(String),
}

impl From<reqwest::Error> for AssistantError {
    fn from(err: reqwest::Error) -> Self {
        AssistantError::RequestError(err)
    }
}

impl From<serde_json::Error> for AssistantError {
    fn from(err: serde_json::Error) -> Self {
        AssistantError::JsonError(err)
    }
}

pub struct Assistant {
    model: String,
    client: reqwest::Client,
    toolchain: Toolchain,
    conversation: Vec<HashMap<String, Value>>,
    progress_callback: Option<ProgressCallback>,
}

impl std::fmt::Debug for Assistant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Assistant")
            .field("model", &self.model)
            .field("toolchain", &self.toolchain)
            .field("conversation_length", &self.conversation.len())
            .field("has_progress_callback", &self.progress_callback.is_some())
            .finish()
    }
}

impl Assistant {
    pub fn new(model: String) -> Self {
        let mut conversation = Vec::new();

        // Add system message
        conversation.push(HashMap::from([
            ("role".into(), Value::String("system".into())),
            ("content".into(), Value::String(
                "You are a coding assistant. Help developers by exploring their codebase.

MANDATORY WORKFLOW:
1. Call pwd to see current directory
2. Call list_directory to see files
3. Read relevant files
4. Provide answer

EXACT JSON FORMAT REQUIRED:

Step 1 - Get current directory:
{\"tool_calls\": [{\"function\": {\"name\": \"pwd\"}}]}

Step 2 - List directory:
{\"tool_calls\": [{\"function\": {\"name\": \"list_directory\", \"arguments\": {\"path\": \".\"}}}]}

Step 3 - Read file:
{\"tool_calls\": [{\"function\": {\"name\": \"read_file\", \"arguments\": {\"path\": \"filename.ext\"}}}]}

Step 4 - Final answer:
{\"content\": \"your answer here\"}

CRITICAL: Use \"arguments\" not \"parameters\". Follow this exact JSON structure.".into()
            )),
        ]));

        Self {
            model,
            client: reqwest::Client::new(),
            toolchain: Toolchain::default(),
            conversation,
            progress_callback: None,
        }
    }

    pub fn with_progress_callback(mut self, callback: ProgressCallback) -> Self {
        self.progress_callback = Some(callback);
        self
    }

    #[tracing::instrument(skip(self))]
    pub async fn ask(&mut self, question: &str) -> Result<String, AssistantError> {
        // Add user message
        self.conversation.push(HashMap::from([
            ("role".into(), Value::String("user".into())),
            ("content".into(), Value::String(question.to_string())),
        ]));

        // Process until we get a final answer (with safety limit)
        let mut loop_count = 0;
        loop {
            loop_count += 1;
            if loop_count > 10 {
                return Err(AssistantError::ToolError("Too many iterations, model not providing final answer".into()));
            }
            // println!("BEFORE GET_MODEL_RESPONSE");
            // println!("{:?}", self.conversation);
            let response = self.get_model_response().await?;

            if let Some(tool_calls) = response.get("tool_calls") {
                // Add the assistant's tool call message to conversation
                self.conversation.push(HashMap::from([
                    ("role".into(), Value::String("assistant".into())),
                    ("tool_calls".into(), tool_calls.clone()),
                ]));
                
                // Execute tools and add results
                self.execute_tools(tool_calls).await?;
                // Continue loop to get model's response to tool results
                continue;
            } else if let Some(content) = response.get("content") {
                // Got final answer
                self.conversation.push(HashMap::from([
                    ("role".into(), Value::String("assistant".into())),
                    ("content".into(), content.clone()),
                ]));
                return Ok(content.as_str().unwrap_or("").to_string());
            } else {
                return Err(AssistantError::ToolError(format!(
                    "Invalid response format: {:?}",
                    response
                )));
            }
        }
    }

    #[tracing::instrument(skip(self))]
    async fn get_model_response(&self) -> Result<HashMap<String, Value>, AssistantError> {
        let response = self
            .client
            .post("http://localhost:11434/api/chat")
            .json(&json!({
                "model": self.model,
                "messages": self.conversation,
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "list_directory",
                            "description": "List files and directories",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "path": {
                                        "type": "string",
                                        "description": "Directory path"
                                    }
                                },
                                "required": ["path"]
                            }
                        }
                    },
                    {
                        "type": "function",
                        "function": {
                            "name": "read_file",
                            "description": "Read file contents",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "path": {
                                        "type": "string",
                                        "description": "File path"
                                    }
                                },
                                "required": ["path"]
                            }
                        }
                    },
                    {
                        "type": "function",
                        "function": {
                            "name": "pwd",
                            "description": "Returns the full path of the current directory"
                        }
                    }
                ],
                "stream": false,  // Key change: no streaming
                "format": "json",
                "options": {
                    "temperature": 0.5
                }
            }))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        // Extract the response message
        let message = response["message"]
            .as_object()
            .ok_or(AssistantError::ToolError(format!(
                "{} - No message in response",
                response
            )))?;

        Ok(message
            .into_iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    #[tracing::instrument(skip(self))]
    async fn execute_tools(&mut self, tool_calls: &Value) -> Result<(), AssistantError> {
        let calls = tool_calls.as_array().ok_or(AssistantError::ToolError(
            "Invalid tool_calls format".into(),
        ))?;

        for call in calls {
            let function = &call["function"];
            let name = function["name"]
                .as_str()
                .ok_or(AssistantError::ToolError("Missing function name".into()))?;
            let args = &function["arguments"];

            let result = match name {
                "list_directory" => {
                    let path = args["path"].as_str().unwrap_or(".");
                    if let Some(ref callback) = self.progress_callback {
                        callback(&format!("📁 Listing directory: {}", path));
                    }
                    let result = self.toolchain
                        .call(Tool::ReadDirectory(path.to_string()))
                        .map_err(|e| {
                            AssistantError::ToolError(format!(
                                "Path: {} - Error: {}",
                                path,
                                e.to_string()
                            ))
                        })?;
                    if let Some(ref callback) = self.progress_callback {
                        callback(&format!("   Found {} items", result.lines().count()));
                    }
                    result
                }
                "read_file" => {
                    let path = args["path"].as_str().unwrap_or(".");
                    if let Some(ref callback) = self.progress_callback {
                        callback(&format!("📄 Reading file: {}", path));
                    }
                    let result = self.toolchain
                        .call(Tool::ReadFile(path.to_string()))
                        .map_err(|e| {
                            AssistantError::ToolError(format!(
                                "Path: {} - Error: {}",
                                path,
                                e.to_string()
                            ))
                        })?;
                    if let Some(ref callback) = self.progress_callback {
                        callback(&format!("   Read {} characters", result.len()));
                    }
                    result
                }
                "pwd" => {
                    if let Some(ref callback) = self.progress_callback {
                        callback("📍 Getting current directory...");
                    }
                    let result = self
                        .toolchain
                        .call(Tool::CurrentDir)
                        .map_err(|e| AssistantError::ToolError(e.to_string()))?;
                    if let Some(ref callback) = self.progress_callback {
                        callback(&format!("   Current directory: {}", result.trim()));
                    }
                    result
                }
                _ => return Err(AssistantError::ToolError(format!("Unknown tool: {}", name))),
            };

            // Add tool result to conversation
            self.conversation.push(HashMap::from([
                ("role".into(), Value::String("tool".into())),
                ("content".into(), Value::String(result)),
                ("name".into(), Value::String(name.to_string())),
            ]));
        }

        Ok(())
    }
}
