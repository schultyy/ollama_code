use std::fs;

pub fn available_tools() -> Vec<&'static str> {
    vec!["list_directory", "read_file"]
}

pub fn read_file(path: &str) -> Result<String, std::io::Error> {
    fs::read_to_string(path)
}

pub fn list_directory(base_path: &str) -> Result<String, std::io::Error> {
    let paths = fs::read_dir(base_path)?;
    let mut results = vec![];

    for path in paths {
        results.push(format!("Name: {}", path.unwrap().path().display()));
    }

    Ok(results.join("\n").to_string())
}
