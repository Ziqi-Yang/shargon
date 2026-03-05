# Repository Guidelines

## Project Structure & Module Organization
This repository is a Rust workspace for Shargon (Shadow Dargon), with crates under `crates/` and shared workspace settings in `Cargo.toml`.

- `crates/shargon-client`: CLI entrypoint (`src/main.rs`) and command traits (`src/cli_command/`).
- `crates/shargon-daemon`: daemon/service binary.
- `crates/shargon-backend`, `shargon-protocol`, `shargon-settings`, `shargon-qemu`, `shargon-nspawn`: library crates for core domains and backend integrations.
- `docs/`: design and infrastructure notes (for example `docs/nix-qemu.md`).

## Build, Test, and Development Commands
Use Cargo from the repository root.

- `cargo build`: build the default workspace member (`shargon-client`).
- `cargo build --workspace`: build all crates.
- `cargo test --workspace`: run all unit tests across crates.
- `cargo run -p shargon-client`: run the CLI locally.
- `cargo run -p shargon-daemon`: run the daemon locally.
- `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings`: format and lint before opening a PR.

## Coding Style & Naming Conventions
- Follow Rust defaults: 4-space indentation, `snake_case` for functions/modules, `PascalCase` for types/traits, `SCREAMING_SNAKE_CASE` for constants.
- Keep crate responsibilities narrow; put reusable logic in `lib.rs` and binary startup/orchestration in `main.rs`.
- Prefer small modules and explicit trait boundaries (for example the `CliCommand` trait pattern).

## Testing Guidelines
- Place unit tests inline with code using `#[cfg(test)] mod tests`.
- Name tests by behavior (for example `creates_vm_snapshot`, `rejects_invalid_config`).
- Run `cargo test --workspace` before commit; add tests for every bug fix or new public behavior.

## Commit & Pull Request Guidelines
use conventional commits style, for example `feat(client): add command dispatcher`.

- Use concise, imperative commit subjects, ideally scoped
- Keep commits focused; avoid mixing refactors with behavior changes.
- PRs should include: purpose, key changes, test evidence (`cargo test`/`clippy` output), and linked issues.
- For user-facing CLI changes, include sample command/output in the PR description.
