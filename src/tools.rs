use std::{
    env, fs,
    path::{self, PathBuf},
};

#[derive(Debug, Clone, PartialEq)]
pub enum Tool {
    ReadDirectory(String),
    ReadFile(String),
    CurrentDir,
}

#[derive(Default, Debug)]
pub struct Toolchain;

impl Toolchain {
    #[tracing::instrument(skip(self))]
    fn normalize_path(&self, abs_or_relative_path: &str) -> Result<PathBuf, std::io::Error> {
        let path = PathBuf::from(abs_or_relative_path);
        std::fs::canonicalize(&path).and_then(|p| path::absolute(p))
    }

    #[tracing::instrument(skip(self))]
    pub fn call(&self, tool: Tool) -> Result<String, std::io::Error> {
        match tool {
            Tool::ReadDirectory(path) => self
                .normalize_path(&path)
                .and_then(|p| self.list_directory(&p)),
            Tool::ReadFile(path) => self.normalize_path(&path).and_then(|p| self.read_file(&p)),
            Tool::CurrentDir => self.pwd(),
        }
    }

    #[tracing::instrument(skip(self))]
    fn pwd(&self) -> Result<String, std::io::Error> {
        env::current_dir().and_then(|path| Ok(path.to_string_lossy().to_string()))
    }

    #[tracing::instrument(skip(self))]
    fn read_file(&self, path: &PathBuf) -> Result<String, std::io::Error> {
        fs::read_to_string(path)
    }

    #[tracing::instrument(skip(self))]
    fn list_directory(&self, base_path: &PathBuf) -> Result<String, std::io::Error> {
        let paths = fs::read_dir(base_path)?;
        let mut dirs = vec![];
        let mut files = vec![];

        for entry in paths {
            let entry = entry?;
            let path = entry.path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();

            if path.is_dir() {
                dirs.push(format!("{}/", name));
            } else {
                let extension = path
                    .extension()
                    .map(|ext| format!(".{}", ext.to_string_lossy()))
                    .unwrap_or_else(|| String::new());
                files.push(format!("{}{}", name, {
                    let display_extension = if extension.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", extension)
                    };
                    display_extension
                }));
            }
        }

        let mut result = String::new();
        result.push_str(&format!("Directory: {}\n\n", base_path.display()));

        if !dirs.is_empty() {
            result.push_str("DIRECTORIES:\n");
            dirs.sort();
            for dir in dirs {
                result.push_str(&format!("{}\n", dir));
            }
            result.push_str("\n");
        }

        if !files.is_empty() {
            result.push_str("FILES:\n");
            files.sort();
            for file in files {
                result.push_str(&format!("{}\n", file));
            }
            result.push_str("\n");
        }

        result
            .push_str("NEXT: Read relevant files to understand the project structure and purpose.");

        Ok(result)
    }
}
