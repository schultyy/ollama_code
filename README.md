# ollama_code

A coding assistant with ollama as LLM backend. This is very experimental software.

The premise is to have a full replacement for Claude Code, backed by Ollama and an LLM that runs on the local machine.

## Requirements

For this to work, you need a computer with a dedicated graphics card.

Confirmed to work:

- Apple Computers with M-series chipsets

Other requirements:

- Ollama
- Rust 1.86 or later

## Usage

To get started, you need to pull `llama3.1:8b` first:

```
$ ollama pull llama3.1:8b
```

Once the model is downloaded, run it. You'll get a prompt:

```
cargo run
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.21s
     Running `target/debug/ollama_code`
Let's get started. Press [ESC] to exit.
? Prompt : Tell me about this codebase
📁 Listing directory: .
   Found 15 items
📄 Reading file: Cargo.toml
   Read 1360 characters
📄 Reading file: README.md
   Read 61 characters

Based on the files I found, Cargo.toml and README.md, it appears that this codebase is for a Rust project called 'ollama_code' which uses an ollama backend. The project has several dependencies including clap, reqwest, serde, and tracing among others. It seems to be designed for building a coding assistant with an ollama LLM (Large Language Model) as the backend.
```

## Limitations

The biggest limitation is the graphics card and its VRAM. The default model here right now is llama3.1:8b to ensure it'll work on machines with 16GB of RAM.
Additionally, we can only use Open Source models that support tool calling.

Product limitations:

- No write access to your codebase (yet)
- It doesn't have as much memory as Claude Code - you cannot build infinitely long conversations.
