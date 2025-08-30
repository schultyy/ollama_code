use std::process::exit;

use color_eyre::Result;
use preflight_ui::PreflightUI;
use ui::App;

mod ollama;
mod preflight_ui;
mod ui;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let terminal = ratatui::init();
    let preflight_ui = PreflightUI::new();

    let preflight_result = preflight_ui.run_checks(terminal).await?;

    if let Err(err) = preflight_result {
        ratatui::restore();
        println!("ollama unavailable: {}", err);
        exit(1);
    }
    ratatui::restore();

    let terminal = ratatui::init();
    let app_result = App::new().run(terminal).await;
    ratatui::restore();
    app_result
}
