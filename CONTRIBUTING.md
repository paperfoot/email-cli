# Contributing to Email CLI

Thanks for your interest in contributing.

## Getting Started

1. Fork the repository
2. Clone your fork:
   ```bash
   git clone https://github.com/<your-username>/email-cli.git
   cd email-cli
   ```
3. Build from source:
   ```bash
   cargo build
   ```

## Development Requirements

- Rust 1.85+ (edition 2024)
- A Resend API key for integration testing

## Making Changes

1. Create a branch for your change:
   ```bash
   git checkout -b your-feature-name
   ```
2. Make your changes
3. Run `cargo clippy` and `cargo fmt` before committing
4. Write clear commit messages that explain *why*, not just *what*

## Pull Requests

- Keep PRs focused on a single change
- Include a clear description of what the PR does and why
- Make sure `cargo check` and `cargo clippy` pass
- Update documentation if your change affects user-facing behavior

## Code Style

- Follow standard Rust conventions
- Use `anyhow` for error handling
- Keep functions small and focused
- Structured JSON output for all commands (see `output.rs`)

## Reporting Issues

Open an issue on GitHub with:
- What you expected to happen
- What actually happened
- Steps to reproduce
- Your OS and Rust version

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
