use crate::{
    constants::{ASSISTANT, CONTENT, ROLE, SYSTEM, SYSTEM_PROMPT, TOOL_CALLS, USER},
    tools::{Tool, Toolchain},
};
use reqwest;
use serde_json::{Value, json};
use std::{collections::HashMap, fmt::Display};

pub type ProgressCallback = Box<dyn Fn(&str) + Send + Sync>;

#[derive(Debug)]
pub enum AssistantError {
    RequestError(reqwest::Error),
    JsonError(serde_json::Error),
    ToolError(String),
}

impl Display for AssistantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssistantError::RequestError(error) => write!(f, "{}", error),
            AssistantError::JsonError(error) => write!(f, "{}", error),
            AssistantError::ToolError(error) => write!(f, "{}", error),
        }
    }
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
            (ROLE.into(), Value::String(SYSTEM.into())),
            (CONTENT.into(), Value::String(SYSTEM_PROMPT.to_string())),
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
            (ROLE.into(), Value::String(USER.into())),
            (CONTENT.into(), Value::String(question.to_string())),
        ]));

        // Process until we get a final answer (with safety limit)
        let mut loop_count = 0;
        loop {
            loop_count += 1;
            if loop_count > 10 {
                return Err(AssistantError::ToolError(
                    "Too many iterations, model not providing final answer".into(),
                ));
            }
            let response = self.get_model_response().await?;

            if let Some(tool_calls) = response.get(TOOL_CALLS) {
                // Add the assistant's tool call message to conversation
                self.conversation.push(HashMap::from([
                    (ROLE.into(), Value::String(ASSISTANT.into())),
                    (TOOL_CALLS.into(), tool_calls.clone()),
                ]));

                // Execute tools and add results
                self.execute_tools(tool_calls).await?;
                // Continue loop to get model's response to tool results
                continue;
            } else if let Some(content) = response.get(CONTENT) {
                // Got final answer
                self.conversation.push(HashMap::from([
                    (ROLE.into(), Value::String(ASSISTANT.into())),
                    (CONTENT.into(), content.clone()),
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
                    },
                    {
                        "type": "function",
                        "function": {
                            "name": "grep",
                            "description": "Searches for a specific substring in a designated file",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "path": {
                                        "type": "string",
                                        "description": "The file to grep through"
                                    },
                                    "search_pattern": {
                                        "type": "string",
                                        "description": "The search pattern"
                                    }
                                },
                                "required": ["path", "search_pattern"]
                            }
                        }
                    }
                ],
                "stream": false,  // Key change: no streaming
                "format": "json",
                "options": {
                    "temperature": 0.5,
                    "num_ctx": 32768  // Use much larger context window
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
                    match self.toolchain.call(Tool::ReadDirectory(path.to_string())) {
                        Ok(result) => {
                            if let Some(ref callback) = self.progress_callback {
                                callback(&format!("   Found {} items", result.lines().count()));
                            }
                            result
                        }
                        Err(e) => {
                            let error_msg = format!("ERROR: Could not list directory '{}' - {}", path, e);
                            if let Some(ref callback) = self.progress_callback {
                                callback(&format!("   ❌ {}", error_msg));
                            }
                            error_msg
                        }
                    }
                }
                "read_file" => {
                    let path = args["path"].as_str().unwrap_or(".");
                    if let Some(ref callback) = self.progress_callback {
                        callback(&format!("📄 Reading file: {}", path));
                    }
                    match self.toolchain.call(Tool::ReadFile(path.to_string())) {
                        Ok(result) => {
                            if let Some(ref callback) = self.progress_callback {
                                callback(&format!("   Read {} characters", result.len()));
                            }
                            result
                        }
                        Err(e) => {
                            let error_msg = format!("ERROR: Could not read file '{}' - {}", path, e);
                            if let Some(ref callback) = self.progress_callback {
                                callback(&format!("   ❌ {}", error_msg));
                            }
                            error_msg
                        }
                    }
                }
                "pwd" => {
                    if let Some(ref callback) = self.progress_callback {
                        callback("📍 Getting current directory...");
                    }
                    match self.toolchain.call(Tool::CurrentDir) {
                        Ok(result) => {
                            if let Some(ref callback) = self.progress_callback {
                                callback(&format!("   Current directory: {}", result.trim()));
                            }
                            result
                        }
                        Err(e) => {
                            let error_msg = format!("ERROR: Could not get current directory - {}", e);
                            if let Some(ref callback) = self.progress_callback {
                                callback(&format!("   ❌ {}", error_msg));
                            }
                            error_msg
                        }
                    }
                }
                "grep" => {
                    let path = args["path"].as_str().ok_or_else(|| {
                        AssistantError::ToolError("Missing Grep Parameter 'path'".into())
                    })?;
                    let search_pattern = args["search_pattern"].as_str().ok_or_else(|| {
                        AssistantError::ToolError("Missing Grep Parameter 'search_pattern'".into())
                    })?;

                    if let Some(ref callback) = self.progress_callback {
                        callback(&format!(
                            "🔍 Searching for '{}' in {}",
                            search_pattern, path
                        ));
                    }

                    match self.toolchain.call(Tool::Grep {
                        search_string: search_pattern.into(),
                        path: path.into(),
                    }) {
                        Ok(result) => {
                            if let Some(ref callback) = self.progress_callback {
                                callback(&format!("   Search completed"));
                            }
                            result
                        }
                        Err(e) => {
                            let error_msg = format!("ERROR: Could not search in file '{}' - {}", path, e);
                            if let Some(ref callback) = self.progress_callback {
                                callback(&format!("   ❌ {}", error_msg));
                            }
                            error_msg
                        }
                    }
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
