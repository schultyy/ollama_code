use std::process::{self, exit};

use clap::Parser;
use cli_prompts::{DisplayPrompt, prompts::Input};
use color_eyre::{Result, owo_colors::OwoColorize};
use tokio::sync::mpsc;

use crate::ollama::OllamaClient;

mod ollama;

#[derive(Parser)]
struct CliArgs {
    ///Which model to use
    #[arg(short, long, default_value = "gpt-oss")]
    pub model: String,

    ///Sets the path to operate in.
    #[arg(short, long, default_value = ".")]
    pub path: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let args = CliArgs::parse();

    if let Err(err) = ollama::check_available(&args.model).await {
        println!("ollama unavailable: {}", err);
        exit(1);
    }

    repl(args).await;

    Ok(())
}

async fn repl(args: CliArgs) {
    println!("Let's get started. Press [ESC] to exit.");
    let (tx, mut rx) = mpsc::channel(1000);
    loop {
        let client = OllamaClient::new(tx.clone(), &args.model);
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
