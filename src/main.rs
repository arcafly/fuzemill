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
    /// Start working on an issue (creates worktree and branch).
    /// If no ID is provided, passes arguments to 'bd create' to generate a new issue.
    Start {
        /// The issue ID (used for branch name)
        #[arg(short, long)]
        id: Option<String>,

        /// The model to use with the AI agent
        #[arg(short, long)]
        model: Option<String>,

        /// AI agent to use: "claude" or "gemini"
        #[arg(short, long, default_value = "claude")]
        agent: String,

        /// Arguments to pass to 'bd create' if no ID is provided
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        create_args: Vec<String>,
    },
    /// Stop working on an issue (removes worktree and branch)
    Unstart {
        /// The issue ID
        issue_id: String,
    },
    /// Merge the PR associated with an issue and pull main
    Merge {
        /// The issue ID
        issue_id: String,
    },
    /// Signal that work is done (closes the Gemini session)
    Done,
    
    /// Test creating a tmux session with Gemini (dry-run without git/beads)
    #[command(hide = true)]
    TestTmux,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Start { id, model, agent, create_args }) => handle_start(id, model, agent, create_args, cli.verbose),
        Some(Commands::Unstart { issue_id }) => handle_unstart(issue_id, cli.verbose),
        Some(Commands::Merge { issue_id }) => handle_merge(issue_id, cli.verbose),
        Some(Commands::Done) => handle_done(cli.verbose),
        Some(Commands::TestTmux) => handle_test_tmux(cli.verbose),
        None => handle_scan(cli.verbose),
    }
}

fn handle_test_tmux(verbose: bool) -> Result<()> {
    let current_dir = env::current_dir()?;
    let session_name = "fuzemill-test";
    println!("Starting test tmux session '{}'...", session_name);
    
    spawn_gemini_tmux(&current_dir, "test-issue", None, session_name, verbose)
}

fn handle_done(verbose: bool) -> Result<()> {
    // Check if we are inside a tmux session
    if let Ok(_) = env::var("TMUX") {
        if verbose {
            println!("Detected tmux session. Killing session...");
        }
        // We kill the session. This will detach the client and close the window.
        Command::new("tmux")
            .arg("kill-session")
            .status()
            .context("Failed to kill tmux session")?;
    } else {
        println!("Not inside a tmux session. 'fuzemill done' currently only works within a fuzemill-started tmux session.");
    }
    Ok(())
}

fn handle_merge(issue_id: String, verbose: bool) -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let git_root = find_git_root(&current_dir).context("Not in a git repository")?;
    let (_, is_worktree) = get_git_common_dir(&git_root)?;

    if is_worktree {
        bail!("'merge' must be run from the main repository, not a worktree.");
    }

    // Try to cleanup the worktree first so the branch is not locked
    let repo_dirname = git_root.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
    let worktree_dir_name = format!("{}-{}", repo_dirname, issue_id);
    let worktree_path = git_root.parent().unwrap_or(Path::new(".")).join(&worktree_dir_name);

    if worktree_path.exists() {
        if verbose {
            println!("Removing worktree at {} to release branch lock...", worktree_path.display());
        }
        let status = Command::new("git")
            .arg("worktree")
            .arg("remove")
            .arg(&worktree_path)
            .status()
            .context("Failed to execute git worktree remove")?;

        if !status.success() {
            eprintln!("Warning: Failed to remove worktree. 'gh pr merge' might fail to delete local branch.");
        }
    }

    if verbose {
        println!("Merging PR for branch '{}'...", issue_id);
    }

    let status = Command::new("gh")
        .arg("pr")
        .arg("merge")
        .arg(&issue_id)
        .arg("--merge")
        .arg("--delete-branch")
        .status()
        .context("Failed to execute 'gh pr merge'")?;

    if !status.success() {
        bail!("Failed to merge PR. Ensure 'gh' is installed and a PR exists for branch '{}'.", issue_id);
    }

    if verbose {
        println!("Pulling latest changes to main...");
    }

    let status = Command::new("git")
        .arg("pull")
        .status()
        .context("Failed to execute 'git pull'")?;

    if !status.success() {
        bail!("Failed to pull to main.");
    }
    
    println!("Successfully merged PR for {} and updated main.", issue_id);

    // Close the bead
    if let Err(e) = close_bead(&git_root, &issue_id, verbose) {
        eprintln!("Warning: Failed to close bead: {}", e);
    }

    Ok(())
}

fn close_bead(cwd: &Path, issue_id: &str, verbose: bool) -> Result<()> {
    if verbose {
        println!("Closing issue {}...", issue_id);
    }
    
    // We assume 'bd close <id>' is the command.
    // If not, we might need 'bd update <id> --status closed'
    // Trying 'bd close' first.
    let output = Command::new("bd")
        .arg("close")
        .arg(issue_id)
        .current_dir(cwd)
        .output()
        .context("Failed to execute 'bd close'")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("bd close failed: {}", stderr.trim());
    }
    Ok(())
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

fn handle_start(id: Option<String>, model: Option<String>, agent: String, create_args: Vec<String>, verbose: bool) -> Result<()> {
    let current_dir = env::current_dir().context("Failed to get current directory")?;
    let git_root = find_git_root(&current_dir).context("Not in a git repository")?;

    let issue_id = if let Some(provided_id) = id {
        check_bead_exists(&provided_id, &git_root, verbose)?;
        provided_id
    } else if !create_args.is_empty() {
        if verbose {
            println!("Creating new issue via 'bd create'...");
        }
        create_new_bead(&create_args, &git_root)?
    } else {
        bail!("Please provide an issue ID via --id or arguments to create a new issue.");
    };
    
    // Determine the main repo name to use for prefixing
    let (main_repo_path, is_worktree) = get_git_common_dir(&git_root)?;
    
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

    // Launch Gemini session
    println!("Launching Gemini session in {}", new_worktree_path.display().to_string().green());
    
    // Update status to hooked
    if let Err(e) = update_bead_status(&git_root, &issue_id, "hooked", verbose) {
        eprintln!("Warning: Failed to set bead status to 'hooked': {}", e);
    }

    let session_name = format!("fuzemill-{}", issue_id);
    match agent.as_str() {
        "gemini" => spawn_gemini_tmux(&new_worktree_path, &issue_id, model, &session_name, verbose)?,
        "claude" => spawn_claude_tmux(&new_worktree_path, &issue_id, model, &session_name, verbose)?,
        _ => bail!("Unknown agent '{}'. Use 'claude' or 'gemini'.", agent),
    }

    // Update status to in_progress
    if let Err(e) = update_bead_status(&git_root, &issue_id, "in_progress", verbose) {
        eprintln!("Warning: Failed to set bead status to 'in_progress': {}", e);
    }

    // Cleanup worktree
    if verbose {
        println!("Cleaning up worktree at {}...", new_worktree_path.display());
    }
    let status = Command::new("git")
        .arg("worktree")
        .arg("remove")
        .arg(&new_worktree_path)
        .status()
        .context("Failed to execute git worktree remove")?;
        
    if !status.success() {
        eprintln!("Warning: Failed to remove worktree at {}", new_worktree_path.display());
    }

    Ok(())
}

fn update_bead_status(cwd: &Path, issue_id: &str, status: &str, verbose: bool) -> Result<()> {
    if verbose {
        println!("Updating bead {} status to '{}'...", issue_id, status);
    }
    
    let output = Command::new("bd")
        .arg("update")
        .arg(issue_id)
        .arg("--status")
        .arg(status)
        .current_dir(cwd)
        .output()
        .context("Failed to execute 'bd update'")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("bd update failed: {}", stderr.trim());
    }
    Ok(())
}

fn spawn_gemini_tmux(path: &Path, issue_id: &str, model: Option<String>, session_name: &str, verbose: bool) -> Result<()> {
    let current_exe = env::current_exe().unwrap_or_else(|_| PathBuf::from("fuzemill"));
    let done_cmd = format!("{} done", current_exe.display());

    let prompt = format!(
        "You are working on issue {}. Please call 'bd show {}' to get the details of the issue. Your task is to fix this issue, commit the changes, push, and open a PR. When committing, please include a descriptive message and add 'Co-authored-by: Gemini <gemini@google.com>' to the commit message. When you are finished, run '{}' to close the session.",
        issue_id, issue_id, done_cmd
    );

    // Construct the command to run inside tmux
    let mut gemini_cmd = String::from("gemini --yolo --prompt-interactive");
    if let Some(m) = model {
        gemini_cmd.push_str(&format!(" --model {}", m));
    }
    // We need to quote the prompt properly for the shell inside tmux
    // A simple escaping for single quotes might be enough if we wrap prompt in single quotes
    let escaped_prompt = prompt.replace("'", "'\\''");
    gemini_cmd.push_str(&format!(" '{}'", escaped_prompt));

    if verbose {
        println!("Creating tmux session '{}'...", session_name);
    }

    // tmux new-session -d -s <name> -c <path> <command>
    let status = Command::new("tmux")
        .arg("new-session")
        .arg("-d")
        .arg("-s")
        .arg(session_name)
        .arg("-c")
        .arg(path)
        .arg(&gemini_cmd)
        .status()
        .context("Failed to create tmux session")?;

    if !status.success() {
        bail!("Failed to create tmux session. Is tmux installed?");
    }

    if verbose {
        println!("Attaching to tmux session...");
    }

    // tmux attach -t <name>
    let _status = Command::new("tmux")
        .arg("attach")
        .arg("-t")
        .arg(session_name)
        .status()
        .context("Failed to attach to tmux session")?;
    
    // If attach fails (e.g. user detaches or session dies), we continue.
    // The start logic will cleanup after this returns.

    Ok(())
}

fn spawn_claude_tmux(path: &Path, issue_id: &str, model: Option<String>, session_name: &str, verbose: bool) -> Result<()> {
    let current_exe = env::current_exe().unwrap_or_else(|_| PathBuf::from("fuzemill"));
    let done_cmd = format!("{} done", current_exe.display());

    let prompt = format!(
        "You are working on issue {}. Please call 'bd show {}' to get the details of the issue. Your task is to fix this issue, commit the changes, push, and open a PR. When committing, please include a descriptive message and add 'Co-authored-by: Claude <noreply@anthropic.com>' to the commit message. When you are finished, run '{}' to close the session.",
        issue_id, issue_id, done_cmd
    );

    let mut claude_cmd = String::from("claude --dangerously-skip-permissions");
    if let Some(m) = model {
        claude_cmd.push_str(&format!(" --model {}", m));
    }
    let escaped_prompt = prompt.replace("'", "'\\''");
    claude_cmd.push_str(&format!(" '{}'", escaped_prompt));

    if verbose {
        println!("Creating tmux session '{}'...", session_name);
    }

    let status = Command::new("tmux")
        .arg("new-session")
        .arg("-d")
        .arg("-s")
        .arg(session_name)
        .arg("-c")
        .arg(path)
        .arg(&claude_cmd)
        .status()
        .context("Failed to create tmux session")?;

    if !status.success() {
        bail!("Failed to create tmux session. Is tmux installed?");
    }

    if verbose {
        println!("Attaching to tmux session...");
    }

    let _status = Command::new("tmux")
        .arg("attach")
        .arg("-t")
        .arg(session_name)
        .status()
        .context("Failed to attach to tmux session")?;

    Ok(())
}

fn create_new_bead(args: &[String], cwd: &Path) -> Result<String> {
    let output = Command::new("bd")
        .arg("create")
        .args(args)
        .arg("--silent")
        .current_dir(cwd)
        .output()
        .context("Failed to execute 'bd create'")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{}", stderr);
        bail!("Failed to create issue.");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let issue_id = stdout.trim().to_string();
    
    if issue_id.is_empty() {
        bail!("'bd create' returned empty issue ID.");
    }
    
    println!("Created issue: {}", issue_id.green());
    Ok(issue_id)
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

    let output = Command::new("bd")
        .arg("show")
        .arg(issue_id)
        .current_dir(cwd)
        .output()
        .context("Failed to execute 'bd' command. Is beads installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("no beads database found") {
             bail!("No beads database found. Run 'bd init' to initialize.");
        } else {
             // It's likely an issue not found error, or some other bd error.
             // We can print the stderr if verbose, or just assume issue not found.
             // Given the request, we treat it as issue not found.
             if verbose {
                 eprintln!("bd error: {}", stderr.trim());
             }
             bail!("Issue '{}' not found.", issue_id);
        }
    }
    
    Ok(())
}