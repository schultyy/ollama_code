use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashMap, fmt::Display};
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

pub async fn check_available() -> Result<(), OllamaError> {
    let mut map = HashMap::new();

    map.insert("model", "gpt-oss");
    map.insert("prompt", "availability-check");

    let client = reqwest::Client::new();
    client
        .post("http://localhost:11434/api/generate")
        .json(&map)
        .send()
        .await?
        .error_for_status()?;

    Ok(())
}

#[derive(Deserialize, Debug)]
pub struct OllamaResponse {
    pub response: Option<String>,
    pub thinking: Option<String>,
    pub done: bool,
}

pub struct OllamaClient {
    tx: Sender<OllamaMessage>,
}

impl OllamaClient {
    pub fn new(tx: Sender<OllamaMessage>) -> Self {
        Self { tx }
    }

    pub async fn prompt_stream(&self, prompt: &str) -> Result<(), OllamaError> {
        let client = reqwest::Client::new();
        let response = client
            .post("http://localhost:11434/api/generate")
            .json(&json!({
                "model": "gpt-oss",
                "prompt": prompt,
                "stream": true
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
            self.tx.send(OllamaMessage::Chunk(chunk)).await?;

            if is_done {
                self.tx.send(OllamaMessage::EOF).await?;
                break;
            }
        }

        Ok(())
    }
}
