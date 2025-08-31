use std::process::{self, exit};

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
    println!("Let's get started. Press [ESC] to exit.");
    loop {
        let input_prompt = Input::new("Prompt ", |s| Ok(s.to_string()));

        let client = OllamaClient::new();

        let prompt_text = input_prompt.display().unwrap_or_else(|_| process::exit(1));

        match client
            .prompt_stream(&prompt_text, |chunk| {
                if let Some(response_text) = &chunk.response {
                    print!("{}", response_text);
                    use std::io::{self, Write};
                    io::stdout().flush().unwrap();
                }
                Ok(())
            })
            .await
        {
            Ok(_) => {
                println!();
            }
            Err(err) => {
                eprintln!("ERR: {}", err);
            }
        }
    }
}
