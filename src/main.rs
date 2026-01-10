use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use colored::*;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Parser)]
#[command(name = "fuzemill")]
#[command(version, about = "Git workflow helper", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start working on an issue (creates worktree and branch)
    Start {
        /// The issue ID (used for branch name)
        issue_id: String,
    },
    /// Stop working on an issue (removes worktree and branch)
    Unstart {
        /// The issue ID
        issue_id: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Start { issue_id }) => handle_start(issue_id, cli.verbose),
        Some(Commands::Unstart { issue_id }) => handle_unstart(issue_id, cli.verbose),
        None => handle_scan(cli.verbose),
    }
}

fn handle_scan(verbose: bool) -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;

    if verbose {
        println!("Scanning from: {}", current_dir.display());
    }

    match find_git_root(&current_dir) {
        Some(git_root) => {
            let repo_name = git_root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            
            println!("{}", repo_name.green().bold());
            
            if verbose {
                println!("Git root found at: {}", git_root.display());
            }
        }
        None => {
            println!("{}", "Not in a git repository".red());
        }
    }

    Ok(())
}

fn handle_start(issue_id: String, verbose: bool) -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let git_root = find_git_root(&current_dir).context("Not in a git repository")?;
    
    // Check if issue exists in beads
    check_bead_exists(&issue_id, &git_root, verbose)?;

    // Determine the main repo name
    let (main_repo_path, is_worktree) = get_git_common_dir(&git_root)?;
    
    // Determine the main repo name to use for prefixing
    let repo_path_for_name = if is_worktree {
        &main_repo_path
    } else {
        &git_root
    };

    let repo_name = repo_path_for_name
        .file_name()
        .and_then(|n| n.to_str())
        .context("Invalid repository path")?;
    
    let base_parent = if is_worktree {
         // If we are in a worktree, we probably want the new worktree to be a sibling of the worktree (and main repo)
         // Usually worktrees are siblings.
         git_root.parent().context("Cannot find parent of git root")?
    } else {
         git_root.parent().context("Cannot find parent of git root")?
    };

    let new_dir_name = format!("{}-{}", repo_name, issue_id);
    let new_worktree_path = base_parent.join(&new_dir_name);

    if new_worktree_path.exists() {
        println!("Worktree directory already exists: {}", new_worktree_path.display());
        println!("Switching context...");
    } else {
        if verbose {
            println!("Creating worktree at: {}", new_worktree_path.display());
        }

        // git worktree add -b <issue_id> <path>
        let status = Command::new("git")
            .arg("worktree")
            .arg("add")
            .arg("-b")
            .arg(&issue_id)
            .arg(&new_worktree_path)
            .status()
            .context("Failed to execute git worktree add")?;

        if !status.success() {
            bail!("git worktree add failed");
        }
    }

    // Run direnv allow if .envrc exists
    if new_worktree_path.join(".envrc").exists() {
        if verbose {
            println!("Detected .envrc, running 'direnv allow'...");
        }
        let _ = Command::new("direnv")
            .arg("allow")
            .current_dir(&new_worktree_path)
            .status();
    }

    // "cd" into the directory by spawning a shell
    println!("Spawning subshell in {}", new_worktree_path.display().to_string().green());
    spawn_shell(&new_worktree_path)?;

    Ok(())
}

fn handle_unstart(issue_id: String, verbose: bool) -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let git_root = find_git_root(&current_dir).context("Not in a git repository")?;
    
    let (main_repo_path, is_worktree) = get_git_common_dir(&git_root)?;

    // Construct expected path for the issue
    // We need to guess where it is. Assuming sibling convention used in Start.
    // If we are IN the worktree to be deleted, we know the path is git_root.
    
    // Case 1: We are inside the worktree we want to delete
    // We verify if the branch matches the issue_id
    let current_branch = get_current_branch()?;
    
    let worktree_to_remove;
    let branch_to_remove = issue_id.clone(); // Assume branch name is issue_id

    if is_worktree && current_branch == issue_id {
        worktree_to_remove = git_root.clone();
        if verbose {
            println!("Detected we are inside the worktree to remove.");
        }
        
        // We need to move out before deleting.
        // Move to main repo.
        env::set_current_dir(&main_repo_path).context("Failed to change directory to main repo")?;
        println!("Moved to main repo: {}", main_repo_path.display());
    } else {
        // Case 2: We are outside (maybe in main), asking to delete a sibling worktree
        // We reconstruct the path
        let repo_dirname = main_repo_path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
        let dir_name = format!("{}-{}", repo_dirname, issue_id);
        // Assuming sibling of main repo
        let probable_path = main_repo_path.parent().unwrap().join(dir_name);
        
        if probable_path.exists() {
            worktree_to_remove = probable_path;
        } else {
            // Try to look it up via 'git worktree list'?
            // For now, fail if not found at expected location
             bail!("Could not find worktree at expected path: {}", probable_path.display());
        }
    }

    if verbose {
        println!("Removing worktree: {}", worktree_to_remove.display());
    }

    // git worktree remove <path>
    let status = Command::new("git")
        .arg("worktree")
        .arg("remove")
        .arg(&worktree_to_remove)
        .status()
        .context("Failed to execute git worktree remove")?;

    if !status.success() {
        // Sometimes force is needed if modified files?
        // For now, let it fail.
        bail!("git worktree remove failed");
    }

    // git branch -D <issue_id>
    let status = Command::new("git")
        .arg("branch")
        .arg("-D")
        .arg(&branch_to_remove)
        .status()
        .context("Failed to delete branch")?;

    if !status.success() {
        println!("{}", "Warning: Failed to delete branch (maybe it was already deleted or different name?)".yellow());
    } else {
        println!("Deleted branch {}", branch_to_remove);
    }
    
    // If we were inside the worktree, we are now in main_repo (due to set_current_dir).
    // We should spawn a shell there so the user feels "cd'ed back".
    if is_worktree && current_branch == issue_id {
        println!("Spawning subshell in {}", main_repo_path.display().to_string().green());
        spawn_shell(&main_repo_path)?;
    }

    Ok(())
}

fn spawn_shell(path: &Path) -> Result<()> {
    let shell = env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    let mut command = Command::new(shell);
    command.current_dir(path);
    
    let status = command.status().context("Failed to spawn shell")?;
    
    if !status.success() {
        bail!("Shell exited with non-zero status");
    }
    Ok(())
}

fn get_current_branch() -> Result<String> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("--abbrev-ref")
        .arg("HEAD")
        .output()
        .context("Failed to get current branch")?;
        
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

// Returns (main_repo_path, is_worktree)
fn get_git_common_dir(git_root: &Path) -> Result<(PathBuf, bool)> {
    // Check if .git is a file (worktree) or dir (main repo)
    let git_item = git_root.join(".git");
    if git_item.is_file() {
        // It's a worktree. 
        // We can find the main dir by parsing the .git file or asking git
        let output = Command::new("git")
            .arg("rev-parse")
            .arg("--path-format=absolute")
            .arg("--git-common-dir")
            .current_dir(git_root)
            .output()
            .context("Failed to get git common dir")?;
            
        let common_dir_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let common_dir = PathBuf::from(common_dir_str);
        
        // common_dir usually points to .git inside main repo. Parent is main repo.
        let main_repo = common_dir.parent().unwrap_or(&common_dir).to_path_buf();
        Ok((main_repo, true))
    } else {
        Ok((git_root.to_path_buf(), false))
    }
}

fn find_git_root(start_path: &Path) -> Option<PathBuf> {
    let mut current_path = start_path;

    loop {
        let git_dir = current_path.join(".git");
        if git_dir.exists() {
            return Some(current_path.to_path_buf());
        }

        match current_path.parent() {
            Some(parent) => current_path = parent,
            None => return None,
        }
    }
}

fn check_bead_exists(issue_id: &str, cwd: &Path, verbose: bool) -> Result<()> {
    if verbose {
        println!("Verifying issue existence for '{}'...", issue_id);
    }

    let status = Command::new("bd")
        .arg("show")
        .arg(issue_id)
        .current_dir(cwd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match status {
        Ok(exit_status) => {
            if !exit_status.success() {
                bail!("Issue '{}' not found in beads (or beads database missing).", issue_id);
            }
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                // bd not installed? 
                // We might want to warn but proceed, or fail?
                // The user explicitly asked for this check, so probably fail or warn.
                // "if there isn't, exit" implies strict check.
                // But if bd is missing, we can't check.
                // Let's assume strict requirement.
                bail!("'bd' command not found. Please install beads to verify issue ID.");
            } else {
                return Err(e).context("Failed to execute 'bd' command");
            }
        }
    }
    
    Ok(())
}