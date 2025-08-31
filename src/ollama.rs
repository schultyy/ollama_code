use serde::Deserialize;
use serde_json::json;
use std::{collections::HashMap, fmt::Display};
use tokio::sync::mpsc::{error::SendError};
use futures_util::StreamExt;
use tokio_stream::wrappers::LinesStream;
use tokio_util::io::StreamReader;
use tokio::io::AsyncBufReadExt;

use reqwest_streams::error::StreamBodyError;

#[derive(Debug)]
pub enum OllamaError {
    ReqwestError(reqwest::Error),
    StreamError(StreamBodyError),
    SendError(SendError<Vec<OllamaResponse>>),
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

impl From<SendError<Vec<OllamaResponse>>> for OllamaError {
    fn from(value: SendError<Vec<OllamaResponse>>) -> Self {
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
    pub model: String,
    pub created_at: String,
    pub response: Option<String>,
    pub thinking: Option<String>,
    pub done: bool,
}

pub struct OllamaClient {}

impl OllamaClient {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn prompt(&self, prompt: &str) -> Result<OllamaResponse, OllamaError> {
        let client = reqwest::Client::new();
        let response = client
            .post("http://localhost:11434/api/generate")
            .json(&json!({
                "model": "gpt-oss",
                "prompt": prompt,
                "stream": false
            }))
            .send()
            .await?
            .json()
            .await?;

        return Ok(response);
    }

    pub async fn prompt_stream<F>(&self, prompt: &str, mut on_chunk: F) -> Result<(), OllamaError> 
    where 
        F: FnMut(&OllamaResponse) -> Result<(), OllamaError>
    {
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
        let stream_reader = StreamReader::new(stream.map(|result| {
            result.map_err(std::io::Error::other)
        }));
        
        let buf_reader = tokio::io::BufReader::new(stream_reader);
        let mut lines = LinesStream::new(buf_reader.lines());

        while let Some(line) = lines.next().await {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            
            let chunk: OllamaResponse = serde_json::from_str(&line)?;
            on_chunk(&chunk)?;
            
            if chunk.done {
                break;
            }
        }

        Ok(())
    }
}
