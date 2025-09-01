use std::process::{self, exit};

use clap::Parser;
use cli_prompts::{DisplayPrompt, prompts::Input};
use color_eyre::{Result, owo_colors::OwoColorize};
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::{Level, event, span};

use crate::{
    ollama::{OllamaClient, ToolCall},
    tools::{list_directory, read_file},
};

mod ollama;
mod otel;
mod tools;

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
    let (tx, mut rx) = mpsc::channel(1000);
    let client = OllamaClient::new(tx.clone(), &args.model);
    let mut show_prompt = true;
    let mut call_stack: usize = 0;
    loop {
        let root_span = span!(Level::INFO, "repl", call_stack = call_stack);
        let _guard = root_span.enter();

        if show_prompt {
            let input_prompt = Input::new("Prompt ", |s| Ok(s.to_string()));
            let prompt_text = input_prompt.display().unwrap_or_else(|_| process::exit(1));

            let user_client = client.clone();
            call_stack += 1;
            tokio::spawn(async move {
                if let Err(err) = user_client.user_prompt(&prompt_text).await {
                    eprintln!("[ERR] Spawn Prompt Failed");
                    eprintln!("[ERR]: {}", err);
                }
            });
        }

        while let Some(response) = rx.recv().await {
            match response {
                ollama::OllamaMessage::Chunk(ollama_response) => {
                    if let Some(tool_calls) = ollama_response.message.tool_calls {
                        let span = tracing::span!(Level::INFO, "TOOL CALL");
                        let _entered = span.enter();
                        for tool in tool_calls.iter() {
                            println!(
                                "\n TOOL CALL {} - {:?}",
                                tool.function.name, tool.function.arguments
                            );
                            tracing::info!(
                                "TOOL_CALL: {} ARGUMENTS: {}",
                                tool.function.name,
                                tool.function.arguments.len()
                            );
                            show_prompt = false;
                            let tool = tool.clone();
                            let client = client.clone();
                            let result = match dispatch_tool(&tool) {
                                Ok(value) => {
                                    event!(Level::INFO, tool = tool.function.name, value = value);
                                    value
                                }
                                Err(err) => {
                                    event!(Level::ERROR, tool = tool.function.name, value = err);
                                    err
                                }
                            };

                            call_stack += 1;
                            tracing::debug!("Increase Call Stack to {}", call_stack);
                            tokio::spawn(async move {
                                if let Err(err) =
                                    client.tool_prompt(&result, &tool.function.name).await
                                {
                                    eprintln!("[ERR] Spawn Tool Prompt Failed");
                                    eprintln!("[ERR]: {}", err);
                                }
                            });
                        }
                    } else if let Some(thinking) = ollama_response.message.thinking {
                        print!("{}", thinking.italic());
                        use std::io::{self, Write};
                        io::stdout().flush().unwrap();
                    } else if let Some(response) = ollama_response.message.content {
                        print!("{}", response);
                        use std::io::{self, Write};
                        io::stdout().flush().unwrap();
                    }
                }
                ollama::OllamaMessage::EOF => {
                    call_stack -= 1;
                    if call_stack == 0 {
                        show_prompt = true;
                    }
                    break;
                }
            }
        }
    }
}

#[tracing::instrument]
fn dispatch_tool(tool: &ToolCall) -> Result<String, String> {
    if tool.function.name == "list_directory" {
        let path = tool
            .function
            .arguments
            .get("path")
            .unwrap_or(&Value::String(".".into()))
            .to_string();
        event!(Level::INFO, path = path);
        return match list_directory(&path) {
            Ok(val) => Ok(val),
            Err(err) => {
                tracing::error!("ERR: {}", err);
                Err(format!("ERR: {}", err))
            }
        };
    } else if tool.function.name == "read_file" {
        let path = tool.function.arguments.get("path").unwrap().to_string();
        event!(Level::INFO, path = path);
        return match read_file(&path) {
            Ok(val) => Ok(val),
            Err(err) => {
                tracing::error!("ERR: {}", err);
                Err(format!("ERR: {}", err))
            }
        };
    }
    return Err(format!("ERR: Tool {} not found", tool.function.name));
}
