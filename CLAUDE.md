# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo build           # Debug build
cargo build --release # Release build
cargo run             # Run in development mode
cargo install --path . # Install globally
```

## What This Project Does

Fuzemill is a Git workflow automation CLI that orchestrates issue-driven development sessions. It integrates:
- **Git worktrees**: Creates isolated working directories per issue
- **Beads (`bd`)**: Git-backed issue tracker for task management
- **Gemini AI**: Launches AI coding sessions in tmux
- **GitHub CLI (`gh`)**: Handles PR merging

## Architecture

Single-file Rust application (`src/main.rs`) using clap for CLI parsing. All logic is in one module with these main flows:

### Commands

- `start --id <issue>` or `start <bd-create-args>`: Creates a worktree + branch for an issue, launches Gemini in tmux, cleans up worktree on exit
- `unstart <issue>`: Removes worktree and branch, spawns shell in main repo if running from within the worktree
- `merge <issue>`: Removes worktree, runs `gh pr merge --delete-branch`, pulls main, closes the bead
- `done`: Kills the current tmux session (used from within a Gemini session)
- No subcommand: Scans for Git root and prints repo name

### Key Functions

- `spawn_gemini_tmux()`: Creates detached tmux session running `gemini --yolo` with a prompt containing the issue ID
- `get_git_common_dir()`: Detects if current directory is a worktree vs main repo
- `update_bead_status()` / `close_bead()`: Updates issue state via `bd` CLI

### External Dependencies

The CLI expects these tools to be available:
- `git` with worktree support
- `bd` (beads CLI) for issue tracking
- `gh` (GitHub CLI) for PR operations
- `gemini` CLI for AI sessions
- `tmux` for session management
- `direnv` (optional, runs `direnv allow` if `.envrc` exists)
