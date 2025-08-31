use std::process::exit;

use cli_prompts::{DisplayPrompt, prompts::Input};
use color_eyre::Result;

use crate::ollama::OllamaClient;

mod ollama;
// mod preflight_ui;
// mod ui;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    if let Err(err) = ollama::check_available().await {
        println!("ollama unavailable: {}", err);
        exit(1);
    }

    repl().await;

    Ok(())
}

async fn repl() {
    loop {
        let input_prompt = Input::new("What are you up to?", |s| Ok(s.to_string()))
            .help_message("Please provide your prompt");

        let client = OllamaClient::new();

        match client.prompt(&input_prompt.display().unwrap()).await {
            Ok(response) => {
                println!("{}", response.response.unwrap_or_default());
            }
            Err(err) => {
                eprintln!("ERR: {}", err);
            }
        }
    }
}
