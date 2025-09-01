use std::{fs, path::PathBuf};

use tracing::Level;

// pub fn available_tools() -> Vec<&'static str> {
//     vec!["list_directory", "read_file"]
// }

#[tracing::instrument]
pub fn read_file(path: &PathBuf) -> Result<String, std::io::Error> {
    fs::read_to_string(path)
}

#[tracing::instrument]
pub fn list_directory(base_path: &PathBuf) -> Result<String, std::io::Error> {
    let paths = fs::read_dir(base_path)?;
    let mut results = vec![];

    for path in paths {
        let format_str = format!("Name: {}", path.unwrap().path().display());
        tracing::event!(Level::INFO, entry = format_str);
        results.push(format_str);
    }

    Ok(results.join("\n").to_string())
}
