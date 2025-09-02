use std::process::exit;

use clap::Parser;
use cli_prompts::{DisplayPrompt, prompts::Input};
use color_eyre::{Result, owo_colors::OwoColorize};
use tokio::sync::mpsc;
use tracing::Level;

use crate::{app::App, ollama::OllamaClient};

mod app;
mod ollama;
mod otel;
mod tools;

#[derive(Parser)]
struct CliArgs {
    ///Which model to use
    #[arg(short, long, default_value = "ollama_code")]
    pub model: String,

    ///Sets the path to operate in.
    #[arg(short, long, default_value = ".")]
    pub path: String,
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
    let (ollama_tx, ollama_rx) = mpsc::channel(1000);
    let (stdout_tx, mut stdout_rx) = mpsc::channel(1000);
    let client = OllamaClient::new(ollama_tx.clone(), &args.model);
    let mut app = App::new(ollama_rx, stdout_tx.clone(), client);

    tokio::spawn(async move {
        while let Some(msg) = stdout_rx.recv().await {
            match msg {
                app::StdoutMessage::Italic(msg) => {
                    print!("{}", msg.italic());
                    use std::io::{self, Write};
                    io::stdout().flush().unwrap();
                }
                app::StdoutMessage::Inline(msg) => {
                    print!("{}", msg);
                    use std::io::{self, Write};
                    io::stdout().flush().unwrap();
                }
                app::StdoutMessage::WithNewLine(msg) => {
                    println!("{}", msg);
                }
                app::StdoutMessage::Error(err) => {
                    eprintln!("{}", err);
                }
            }
        }
    });

    loop {
        let mut prompt_text = None;
        if app.show_prompt() {
            let input_prompt = Input::new("Prompt ", |s| Ok(s.to_string()));
            prompt_text = Some(input_prompt.display().expect("Failed to get input value"));
        }
        match app.repl(prompt_text).await {
            Ok(()) => (),
            Err(err) => {
                eprintln!("[ERR]: {}", err);
            }
        }
    }
}
