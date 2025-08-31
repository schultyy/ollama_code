use std::process::{self, exit};

use clap::Parser;
use cli_prompts::{DisplayPrompt, prompts::Input};
use color_eyre::{Result, owo_colors::OwoColorize};
use serde_json::Value;
use tokio::sync::mpsc;

use crate::{
    ollama::{OllamaClient, Role, ToolCall},
    tools::{list_directory, read_file},
};

mod ollama;
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
    let client = OllamaClient::new(tx.clone(), &args.model);
    let mut show_prompt = true;
    let mut call_stack: usize = 0;
    loop {
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
                        for tool in tool_calls.iter() {
                            println!("TOOL CALL: {:?}", tool);
                            show_prompt = false;
                            let tool = tool.clone();
                            let client = client.clone();
                            let result = dispatch_tool(&tool);

                            call_stack += 1;
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

fn dispatch_tool(tool: &ToolCall) -> String {
    if tool.function.name == "list_directory" {
        let path = tool
            .function
            .arguments
            .get("path")
            .unwrap_or(&Value::String("./".into()))
            .to_string();
        return match list_directory(&path) {
            Ok(val) => val,
            Err(err) => format!("ERR: {}", err),
        };
    } else if tool.function.name == "read_file" {
        let path = tool.function.arguments.get("path").unwrap().to_string();
        return match read_file(&path) {
            Ok(val) => val,
            Err(err) => format!("ERR: {}", err),
        };
    }
    return format!("ERR: Tool {} not found", tool.function.name);
}
