# Fuzemill

A production-grade Rust CLI tool that detects if the current working directory is inside a Git repository.

## Features

- 🚀 Fast and lightweight directory traversal.
- 🎨 Color-coded output for better visibility.
- 🛠️ Simple and robust error handling.

## Installation

Ensure you have Rust and Cargo installed. Then, clone the repository and build the project:

```bash
git clone https://github.com/arcafly/fuzemill.git
cd fuzemill
cargo build --release
```

To install it locally:

```bash
cargo install --path .
```

To uninstall it:

```bash
cargo uninstall fuzemill
```

## Usage

Run the tool from any directory:

```bash
fuzemill
```

### Options

- `-v`, `--verbose`: Enable verbose output to see the scanning path and exact root location.
- `-h`, `--help`: Print help information.
- `-V`, `--version`: Print version information.

### Examples

**Inside a Git repository:**

```bash
$ fuzemill
fuzemill
```

**Outside a Git repository:**

```bash
$ cd /tmp
$ fuzemill
Not in a git repository
```

## Development

Run the project in development mode within the project root:

```bash
cargo run
```

### Testing in other directories

To test the CLI in a different directory without installing it, build the project and call the binary using its absolute path:

1. Build the binary:
   ```bash
   cargo build
   ```
2. Navigate to any other directory and run the binary:
   ```bash
   cd /some/other/repo
   /Users/jbtobar/dev/@arcafly/fuzemill/target/debug/fuzemill
   ```

Alternatively, you can install it globally for the current user:

```bash
cargo install --path .
```