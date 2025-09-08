use std::process::{self, exit};

use clap::Parser;
use cli_prompts::{DisplayPrompt, prompts::Input};
use color_eyre::Result;
use serde_json::Value;
use tracing::Level;

use crate::assistant::Assistant;
mod assistant;
mod constants;
mod ollama;
mod otel;
mod tools;

#[derive(Parser)]
struct CliArgs {
    ///Which model to use
    #[arg(short, long, default_value = "llama3.1:8b")]
    pub model: String,

    ///Sets the path to operate in.
    #[arg(short, long, default_value = ".")]
    pub path: String,
    ///Determines whether to display model's thinking output
    #[arg(short, long, default_value = "false")]
    pub show_thinking: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let _tracer =
        otel::setup_otlp("http://localhost:4317", "ollama_code").expect("Failed to setup OTLP");

    let span = tracing::span!(Level::INFO, "root");
    let _guard = span.enter();

    let args = CliArgs::parse();

    if let Err(err) = ollama::check_available(&args.model).await {
        println!("ollama unavailable: {}", err);
        exit(1);
    }

    repl(args).await;
    opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .build()
        .shutdown()?;

    Ok(())
}

async fn repl(args: CliArgs) {
    println!("Let's get started. Press [ESC] to exit.");
    let mut assistant = Assistant::new(args.model).with_progress_callback(Box::new(|msg| {
        println!("{}", msg);
    }));

    loop {
        let input_prompt = Input::new("Prompt ", |s| Ok(s.to_string()));
        let question = match input_prompt.display() {
            Ok(val) => val,
            Err(_) => process::exit(0),
        };

        match assistant.ask(&question).await {
            Ok(response) => {
                // Try to parse as JSON first, fallback to plain text
                if let Ok(json_val) = serde_json::from_str::<Value>(&response) {
                    if let Some(content) = json_val.get("content").and_then(|c| c.as_str()) {
                        println!("\n{}\n", content);
                    } else {
                        println!("\n{}\n", response);
                    }
                } else {
                    // Plain text response
                    println!("\n{}\n", response);
                }
            }
            Err(err) => eprintln!("[ERR]: {}", err),
        }
    }
}
