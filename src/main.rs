use std::process::{self, exit};

use cli_prompts::{DisplayPrompt, prompts::Input};
use color_eyre::{Result, owo_colors::OwoColorize};
use tokio::sync::mpsc;

use crate::ollama::OllamaClient;

mod ollama;

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
    println!("Let's get started. Press [ESC] to exit.");
    let (tx, mut rx) = mpsc::channel(1000);
    loop {
        let client = OllamaClient::new(tx.clone());
        let input_prompt = Input::new("Prompt ", |s| Ok(s.to_string()));

        let prompt_text = input_prompt.display().unwrap_or_else(|_| process::exit(1));

        tokio::spawn(async move {
            if let Err(err) = client.prompt_stream(&prompt_text).await {
                eprintln!("[ERR] Spawn Prompt Failed");
                eprintln!("[ERR]: {}", err);
            }
        });

        while let Some(response) = rx.recv().await {
            match response {
                ollama::OllamaMessage::Chunk(ollama_response) => {
                    if let Some(thinking) = ollama_response.thinking {
                        print!("{}", thinking.italic());
                        use std::io::{self, Write};
                        io::stdout().flush().unwrap();
                    } else if let Some(response) = ollama_response.response {
                        print!("{}", response);
                        use std::io::{self, Write};
                        io::stdout().flush().unwrap();
                    }
                }
                ollama::OllamaMessage::EOF => break,
            }
        }
    }
}
