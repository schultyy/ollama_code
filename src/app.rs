use std::{collections::HashMap, fmt::Display, path::PathBuf};

use serde_json::Value;
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{Level, event, span};

use crate::{
    constants::ASSISTANT,
    ollama::{self, OllamaClient, OllamaMessage, Role, ToolCall},
    tools::{Tool, Toolchain},
};

pub enum AppError {
    SendError(tokio::sync::mpsc::error::SendError<StdoutMessage>),
}

impl From<tokio::sync::mpsc::error::SendError<StdoutMessage>> for AppError {
    fn from(value: tokio::sync::mpsc::error::SendError<StdoutMessage>) -> Self {
        Self::SendError(value)
    }
}

impl Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::SendError(send_error) => write!(f, "{}", send_error),
        }
    }
}

#[derive(Debug, Clone)]
pub enum StdoutMessage {
    Italic(String),
    Inline(String),
    WithNewLine(String),
    Error(String),
    EOF,
}

#[derive(Debug)]
pub struct App {
    call_stack: usize,
    show_prompt: bool,
    client: OllamaClient,
    stdout_tx: Sender<StdoutMessage>,
    rx: Receiver<OllamaMessage>,
    history: Vec<HashMap<String, Value>>,
}

fn system_prompt() -> HashMap<String, Value> {
    let mut map = HashMap::new();
    map.insert("role".into(), Value::String("system".into()));
    map.insert(
        "content".into(),
        Value::String(crate::constants::SYSTEM_PROMPT.into()),
    );
    map
}

fn user_prompt(prompt_text: &str) -> HashMap<String, Value> {
    HashMap::from([
        ("role".into(), Value::String(Role::User.to_string())),
        ("content".into(), Value::String(prompt_text.to_string())),
    ])
}

fn tool_prompt(content: &str, tool_name: &str) -> HashMap<String, Value> {
    HashMap::from([
        ("role".into(), Value::String(Role::Tool.to_string())),
        ("content".into(), Value::String(content.to_string())),
        ("tool_name".into(), Value::String(tool_name.to_string())),
    ])
}

fn add_assistant_response(content: &str) -> HashMap<String, Value> {
    HashMap::from([
        ("role".into(), Value::String(ASSISTANT.into())),
        ("content".into(), Value::String(content.into())),
    ])
}

impl App {
    pub fn new(
        rx: Receiver<OllamaMessage>,
        stdout_tx: Sender<StdoutMessage>,
        client: OllamaClient,
    ) -> Self {
        Self {
            call_stack: 0,
            show_prompt: true,
            client,
            stdout_tx: stdout_tx,
            rx,
            history: vec![system_prompt()],
        }
    }

    pub async fn repl(&mut self, prompt: Option<String>) -> Result<(), AppError> {
        let root_span = span!(Level::INFO, "repl", call_stack = self.call_stack);
        let _guard = root_span.enter();

        if self.show_prompt {
            let user_client = self.client.clone();
            self.call_stack += 1;

            if prompt.is_none() {
                panic!("Expected Prompt, Got None");
            }

            let user_prompt_stdout = self.stdout_tx.clone();
            self.history.push(user_prompt(&prompt.unwrap()));
            let history = self.history.clone();
            tokio::spawn(async move {
                if let Err(err) = user_client.prompt_stream(&history).await {
                    let msg = format!("[ERR] Spawn Prompt Failed.\n[ERR]: {}", err);
                    if let Err(err) = user_prompt_stdout.send(StdoutMessage::Error(msg)).await {
                        panic!("[ERR] Failed to send message in user prompt: {}", err);
                    }
                }
            });
        }

        let mut assistant_response_buffer: String = "".into();

        while let Some(response) = self.rx.recv().await {
            match response {
                ollama::OllamaMessage::Chunk(ollama_response) => {
                    if let Some(tool_calls) = ollama_response.message.tool_calls {
                        let span = tracing::span!(Level::INFO, "TOOL CALL");
                        let _entered = span.enter();
                        for tool in tool_calls.iter() {
                            self.stdout_tx
                                .send(StdoutMessage::WithNewLine(format!(
                                    "\n TOOL CALL {} - {:?}",
                                    tool.function.name, tool.function.arguments
                                )))
                                .await?;
                            tracing::info!(
                                "TOOL_CALL: {} ARGUMENTS: {}",
                                tool.function.name,
                                tool.function.arguments.len()
                            );
                            self.show_prompt = false;
                            let tool = tool.clone();
                            let client = self.client.clone();
                            let result = match self.dispatch_tool(&tool) {
                                Ok(value) => {
                                    event!(Level::INFO, tool = tool.function.name, value = value);
                                    value
                                }
                                Err(err) => {
                                    event!(Level::ERROR, tool = tool.function.name, value = err);
                                    err
                                }
                            };

                            self.call_stack += 1;
                            tracing::debug!("Increase Call Stack to {}", self.call_stack);
                            let tool_call_stdout = self.stdout_tx.clone();
                            self.history.push(tool_prompt(&result, &tool.function.name));
                            let history = self.history.clone();
                            tokio::spawn(async move {
                                if let Err(err) = client.prompt_stream(&history).await {
                                    let msg =
                                        format!("[ERR] Spawn Tool Prompt Failed\n. [ERR]: {}", err);
                                    if let Err(err) =
                                        tool_call_stdout.send(StdoutMessage::Error(msg)).await
                                    {
                                        panic!(
                                            "[ERR] Failed to send message in spawn tool prompt: {}",
                                            err
                                        );
                                    }
                                }
                            });
                        }
                    } else if let Some(thinking) = ollama_response.message.thinking {
                        self.stdout_tx.send(StdoutMessage::Italic(thinking)).await?;
                    } else if let Some(response) = ollama_response.message.content {
                        assistant_response_buffer =
                            format!("{}{}", assistant_response_buffer, response);
                        self.stdout_tx.send(StdoutMessage::Inline(response)).await?;
                    }
                }
                ollama::OllamaMessage::EOF => {
                    self.call_stack -= 1;
                    if self.call_stack == 0 {
                        self.show_prompt = true;
                        self.stdout_tx.send(StdoutMessage::EOF).await?;
                        self.history
                            .push(add_assistant_response(&assistant_response_buffer));
                    }
                    break;
                }
            }
        }
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    fn dispatch_tool(&self, tool: &ToolCall) -> Result<String, String> {
        let toolchain = Toolchain::default();
        if tool.function.name == "list_directory" {
            let path = tool
                .function
                .arguments
                .get("path")
                .unwrap_or(&Value::String(".".into()))
                .to_string();

            return match toolchain.call(Tool::ReadDirectory(path)) {
                Ok(val) => Ok(val),
                Err(err) => {
                    tracing::error!("ERR: {}", err);
                    Err(format!("ERR: {}", err))
                }
            };
        } else if tool.function.name == "read_file" {
            let path = tool
                .function
                .arguments
                .get("path")
                .unwrap_or(&Value::String(".".into()))
                .to_string();

            return match toolchain.call(Tool::ReadFile(path)) {
                Ok(val) => Ok(val),
                Err(err) => {
                    tracing::error!("ERR: {}", err);
                    Err(format!("ERR: {}", err))
                }
            };
        }
        return Err(format!("ERR: Tool {} not found", tool.function.name));
    }

    pub fn show_prompt(&self) -> bool {
        self.show_prompt
    }
}
