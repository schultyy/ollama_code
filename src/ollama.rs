use serde::Deserialize;
use serde_json::json;
use std::{collections::HashMap, fmt::Display};
use tokio::sync::mpsc::{self, Sender, error::SendError};

use reqwest_streams::error::StreamBodyError;

#[derive(Debug)]
pub enum OllamaError {
    ReqwestError(reqwest::Error),
    StreamError(StreamBodyError),
    SendError(SendError<Vec<OllamaResponse>>),
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

impl Display for OllamaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OllamaError::ReqwestError(error) => write!(f, "{}", error),
            OllamaError::StreamError(stream_body_error) => write!(f, "{}", stream_body_error),
            OllamaError::SendError(send_error) => write!(f, "{}", send_error),
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
                "json": true,
                "prompt": prompt,
                "stream": false
            }))
            .send()
            .await?
            .json()
            .await?;

        return Ok(response);
    }
}
