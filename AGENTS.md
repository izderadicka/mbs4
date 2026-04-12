# Repository Guidelines

## Project Structure & Module Organization
This repository is a Rust workspace. Root-level files such as `Cargo.toml`, `README.md`, `migrations/`, and `test-data/` define shared configuration, SQL schema changes, and sample assets used by tests.

Crates live under `crates/`:
- `mbs4-server`: server binary and startup wiring
- `mbs4-app`: application logic and REST handlers
- `mbs4-dal`: SQLx-based data access layer
- `mbs4-types`, `mbs4-auth`, `mbs4-store`, `mbs4-search`, `mbs4-image`, `mbs4-calibre`: shared domain and service modules
- `mbs4-cli`: admin and maintenance commands
- `mbs4-e2e-tests`: end-to-end integration tests

Keep new code in the most specific crate possible. Put crate-local tests next to code with `#[cfg(test)]`; broader integration tests belong in each crate’s `tests/` directory.

## Build, Test, and Development Commands
- `cargo build --workspace`: build all crates
- `cargo test --workspace`: run unit and integration tests across the workspace
- `cargo test -p mbs4-e2e-tests`: run end-to-end server tests
- `cargo run -p mbs4-server -- --data-dir test-data --cors`: run the server locally
- `cargo run -p mbs4-cli -- --help`: inspect CLI operations
- `cargo fmt --all` and `cargo clippy --workspace --all-targets --all-features`: format and lint before submitting

Helper scripts already in the repo:
- `run.sh`: local server startup
- `watch.sh`: live-reload server development with `cargo watch`

## Coding Style & Naming Conventions
Follow standard Rust style: 4-space indentation, `snake_case` for functions/modules/files, `CamelCase` for types, and small focused modules. Prefer explicit types at API boundaries and `Result`-based error propagation over panics in application code. Use `cargo fmt` as the formatting source of truth.

## Testing Guidelines
Tests use Rust’s built-in test framework with `#[test]` and `#[tokio::test]`. Name tests by behavior, for example `test_health` or `test_bookshelf_crud_and_items`. Database-related tests should apply the shared SQL migrations and use assets from `test-data/samples/` where appropriate.

## Commit & Pull Request Guidelines
Recent history uses short, imperative subjects such as `Add readme` and `clippy - third round`. Keep commit messages concise and action-oriented.

Always run `cargo fmt` for changed files.

For pull requests, include:
- a short description of the behavior change
- linked issue or context when relevant
- test coverage notes (`cargo test ...`, manual API checks, or both)
- screenshots only when UI or Swagger-visible behavior changes

## Configuration & Environment Tips
Docker users should start from `docker-compose-template.yml`. Local conversion features depend on Calibre command-line tools, and some document conversions also require LibreOffice.
