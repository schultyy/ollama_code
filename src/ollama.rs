use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fmt::Display;
use tokio::io::AsyncBufReadExt;
use tokio::sync::mpsc::Sender;
use tokio::sync::mpsc::error::SendError;
use tokio_stream::wrappers::LinesStream;
use tokio_util::io::StreamReader;
use tracing::Level;

use reqwest_streams::error::StreamBodyError;

#[derive(Debug)]
pub enum OllamaError {
    ReqwestError(reqwest::Error),
    StreamError(StreamBodyError),
    SendError(SendError<OllamaMessage>),
    JsonError(serde_json::Error),
    IoError(std::io::Error),
}

impl From<StreamBodyError> for OllamaError {
    fn from(value: StreamBodyError) -> Self {
        OllamaError::StreamError(value)
    }
}

impl From<reqwest::Error> for OllamaError {
    fn from(value: reqwest::Error) -> Self {
        OllamaError::ReqwestError(value)
    }
}

impl From<SendError<OllamaMessage>> for OllamaError {
    fn from(value: SendError<OllamaMessage>) -> Self {
        Self::SendError(value)
    }
}

impl From<serde_json::Error> for OllamaError {
    fn from(value: serde_json::Error) -> Self {
        OllamaError::JsonError(value)
    }
}

impl From<std::io::Error> for OllamaError {
    fn from(value: std::io::Error) -> Self {
        OllamaError::IoError(value)
    }
}

impl Display for OllamaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OllamaError::ReqwestError(error) => write!(f, "{}", error),
            OllamaError::StreamError(stream_body_error) => write!(f, "{}", stream_body_error),
            OllamaError::SendError(send_error) => write!(f, "{}", send_error),
            OllamaError::JsonError(json_error) => write!(f, "{}", json_error),
            OllamaError::IoError(io_error) => write!(f, "{}", io_error),
        }
    }
}

pub enum OllamaMessage {
    Chunk(OllamaResponse),
    EOF,
}

pub async fn check_available(model: &str) -> Result<(), OllamaError> {
    let client = reqwest::Client::new();
    let messages: Vec<serde_json::Value> = vec![];
    client
        .post("http://localhost:11434/api/chat")
        .json(&json!({
            "model": model,
            "messages": messages
        }))
        .send()
        .await?
        .error_for_status()?;

    Ok(())
}

#[derive(Debug, Deserialize, Clone)]
pub struct Function {
    pub name: String,
    pub arguments: HashMap<String, Value>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ToolCall {
    pub function: Function,
}

#[derive(Deserialize, Debug, Clone)]
pub struct OllamaChunk {
    pub content: Option<String>,
    pub thinking: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct OllamaResponse {
    pub done: bool,
    pub message: OllamaChunk,
}

#[derive(Debug, Clone)]
pub struct OllamaClient {
    tx: Sender<OllamaMessage>,
    system_prompt: String,
    model: String,
}

#[derive(Serialize)]
pub enum Role {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "tool")]
    Tool,
}

impl Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::User => write!(f, "user"),
            Role::Tool => write!(f, "tool"),
        }
    }
}

impl OllamaClient {
    pub fn new(tx: Sender<OllamaMessage>, model: &str) -> Self {
        let system_prompt = "
            You are a code assistant designed to help with programming questions and tasks. Your responses should be:

            1. CONCISE - Provide direct, minimal answers without unnecessary elaboration
            2. FOCUSED - Only answer the specific question asked, do not expand scope or suggest additional features
            3. CODE-ONLY - Only work on requests that are related to programming, software development, or technical implementation

            REJECT requests for:
            - General conversation or chitchat
            - Creative writing or storytelling
            - Math problems unrelated to programming
            - General knowledge questions
            - Personal advice or opinions
            - Any non-programming tasks

            For code questions, provide:
            - Direct code solutions without extra context unless specifically requested
            - Brief explanations only when necessary for understanding
            - No suggestions for improvements unless asked
            - No alternative approaches unless specifically requested

            TOOL USAGE:
            - Use file system tools ONLY when explicitly requested or when code changes need to be applied
            - Read files only when necessary to understand the codebase context
            - Write/modify files only when specifically asked to implement changes
            - Do NOT browse directories or examine files unless directly relevant to the question
            - Do NOT automatically run code unless execution is requested
            - Use tools minimally - if you can answer without tools, do so

            You will get invoked as a command line tool in a user's codebase. When you get asked questions about files or the code base, assume you have access to all files in the current directory.
            Make use of the tools provided to you.

            Before using any tool, briefly state what you're going to do and why it's necessary.

            Stay within the bounds of what was asked. Do not anticipate needs, explore codebases unprompted, or offer unsolicited tool usage.
        ";
        Self {
            tx,
            system_prompt: system_prompt.into(),
            model: model.into(),
        }
    }

    #[tracing::instrument(skip(self))]
    pub async fn tool_prompt(&self, content: &str, tool_name: &str) -> Result<(), OllamaError> {
        let mut messages = vec![];
        messages.push(HashMap::from([
            ("role".into(), Value::String(Role::Tool.to_string())),
            ("content".into(), Value::String(content.to_string())),
            ("tool_name".into(), Value::String(tool_name.to_string())),
        ]));
        tracing::event!(Level::INFO, content = content, tool_name = tool_name);

        self.prompt_stream(messages).await
    }

    #[tracing::instrument(skip(self))]
    pub async fn user_prompt(&self, prompt: &str) -> Result<(), OllamaError> {
        let mut messages = vec![];
        let mut map = HashMap::new();
        map.insert("role".into(), Value::String("system".into()));
        map.insert(
            "content".into(),
            Value::String(self.system_prompt.to_string()),
        );
        messages.push(map);

        messages.push(HashMap::from([
            ("role".into(), Value::String(Role::User.to_string())),
            ("content".into(), Value::String(prompt.to_string())),
        ]));

        self.prompt_stream(messages).await
    }

    #[tracing::instrument(skip(self))]
    async fn prompt_stream(
        &self,
        messages: Vec<HashMap<String, Value>>,
    ) -> Result<(), OllamaError> {
        let client = reqwest::Client::new();

        let response = client
            .post("http://localhost:11434/api/chat")
            .json(&json!({
                "model": self.model,
                "messages": messages,
                "tools": [
                    {
                        "type": "function",
                        "function": {
                            "name": "list_directory",
                            "description": "Get all files and directories from the current directory",
                        }
                    },
                    {
                        "type": "function",
                        "function": {
                            "name": "read_file",
                            "description": "Returns the contents of a specific file",
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "path": {
                                        "type": "string",
                                        "description": "The file path"
                                    }
                                },
                                "required": ["path"]
                            }
                        }
                    },
                ],
                "stream": true,
            }))
            .send()
            .await?;

        let stream = response.bytes_stream();
        let stream_reader =
            StreamReader::new(stream.map(|result| result.map_err(std::io::Error::other)));

        let buf_reader = tokio::io::BufReader::new(stream_reader);
        let mut lines = LinesStream::new(buf_reader.lines());

        while let Some(line) = lines.next().await {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let chunk: OllamaResponse = serde_json::from_str(&line)?;
            let is_done = chunk.done;
            self.tx.send(OllamaMessage::Chunk(chunk.clone())).await?;

            if is_done {
                self.tx.send(OllamaMessage::EOF).await?;
                break;
            }
        }

        Ok(())
    }
}
