# Repository Guidelines

## Project Structure & Module Organization
`shargon` is a Rust workspace rooted at [`Cargo.toml`](/home/meowking/proj/personal/shargon/Cargo.toml). Code lives under [`crates/`](/home/meowking/proj/personal/shargon/crates): `shargon-client` is the default CLI, `shargon-daemon` hosts the long-running service, `shargon-protocol` contains the gRPC/protobuf layer, and shared or backend-specific logic sits in crates such as `shargon-settings`, `shargon-version`, `shargon-qemu`, and `shargon-nspawn`. Protocol definitions live in [`crates/shargon-protocol/proto/`](/home/meowking/proj/personal/shargon/crates/shargon-protocol/proto), and repo docs live in [`docs/`](/home/meowking/proj/personal/shargon/docs).

## Build, Test, and Development Commands
Use Nix when you need the pinned toolchain support:

- `nix develop` opens a shell with `protobuf` available for proto builds.
- `cargo build --workspace` builds all crates.
- `cargo run -p shargon-client -- version` runs the client CLI.
- `cargo run -p shargon-daemon -- run` starts the daemon entrypoint.
- `cargo test --workspace` runs all unit tests.
- `cargo fmt --all` formats the workspace.
- `cargo clippy --workspace --all-targets` checks for common Rust issues.

## Coding Style & Naming Conventions
Follow standard Rust formatting via `rustfmt` with 4-space indentation. Keep crate names kebab-case (`shargon-client`), modules and functions snake_case, and types/traits PascalCase. Prefer small modules under `src/` and keep CLI command implementations grouped under `cli_command/`. When editing protobuf-backed code, update the `.proto` file first and let Cargo rebuild generated bindings through `build.rs`.

## Testing Guidelines
Current tests are lightweight inline unit tests in crate `src/lib.rs` files. For new logic, add focused unit tests next to the code with `#[cfg(test)] mod tests`; add integration tests under `crates/<crate>/tests/` when behavior crosses module boundaries. Name tests after behavior, for example `returns_version_from_workspace_crate`. Run `cargo test --workspace` before opening a PR.

## Commit & Pull Request Guidelines
Recent history follows a Conventional Commit style such as `feat(protocol): ...`, `feat: ...`, `refactor: ...`, and `doc: ...`. Use imperative subjects and add a scope when the change is isolated to one crate. PRs should include a short summary, list affected crates, note any protocol or socket-path changes, and include the verification commands you ran. Link the issue when one exists.
