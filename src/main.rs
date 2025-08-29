use std::process::exit;

use color_eyre::Result;
use ui::App;

use crate::ollama::OllamaError;

mod ollama;
mod ui;

async fn preflight_checks() -> Result<(), OllamaError> {
    println!("Check if ollama is available");
    ollama::check_available().await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    if let Err(err) = preflight_checks().await {
        println!("ollama unavailable: {}", err);
        exit(1)
    }

    color_eyre::install()?;
    let terminal = ratatui::init();
    let app_result = App::new().run(terminal);
    ratatui::restore();
    app_result
}
