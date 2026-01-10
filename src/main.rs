use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "fuzemill")]
#[command(version, about = "Detects if the current directory is within a git repository", long_about = None)]
struct Cli {
    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let current_dir = env::current_dir().context("Failed to get current directory")?;

    if cli.verbose {
        println!("Scanning from: {}", current_dir.display());
    }

    match find_git_root(&current_dir) {
        Some(git_root) => {
            let repo_name = git_root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            
            println!("{}", repo_name.green().bold());
            
            if cli.verbose {
                println!("Git root found at: {}", git_root.display());
            }
        }
        None => {
            println!("{}", "Not in a git repository".red());
        }
    }

    Ok(())
}

fn find_git_root(start_path: &Path) -> Option<PathBuf> {
    let mut current_path = start_path;

    loop {
        let git_dir = current_path.join(".git");
        if git_dir.exists() {
            // It could be a directory (normal repo) or a file (worktree/submodule)
            return Some(current_path.to_path_buf());
        }

        match current_path.parent() {
            Some(parent) => current_path = parent,
            None => return None,
        }
    }
}