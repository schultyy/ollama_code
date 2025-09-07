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
        Self {
            tx,
            model: model.into(),
        }
    }

    #[tracing::instrument(skip(self))]
    pub async fn prompt_stream(
        &self,
        messages: &Vec<HashMap<String, Value>>,
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
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "path": {
                                        "type": "string",
                                        "description": "The directory path to read. Can be \".\" for the current directory."
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
                "think": false,
                // "format": "json",
                "keep_alive": "10m",
                // "options": {
                //    "temperature": 0.9,
                //    "top_p": 0.9,
                //    "repeat_penalty": 1.5
                //  }
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

            match serde_json::from_str::<OllamaResponse>(&line) {
                Ok(chunk) => {
                    let is_done = chunk.done;
                    self.tx.send(OllamaMessage::Chunk(chunk.clone())).await?;

                    if is_done {
                        self.tx.send(OllamaMessage::EOF).await?;
                        break;
                    }
                }
                Err(err) => {
                    tracing::error!("LINE: {}", line);
                    tracing::error!("ERROR: {}", err);
                    return Err(err.into());
                }
            }
        }

        Ok(())
    }
}
