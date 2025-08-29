use std::{collections::HashMap, fmt::Display};

pub enum OllamaError {
    ReqwestError(reqwest::Error),
}

impl From<reqwest::Error> for OllamaError {
    fn from(value: reqwest::Error) -> Self {
        OllamaError::ReqwestError(value)
    }
}

impl Display for OllamaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OllamaError::ReqwestError(error) => write!(f, "{}", error),
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
