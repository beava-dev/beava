# Contributing to Beava

Thanks for your interest in Beava. This guide covers the local build, test, and pull-request workflow. For what Beava *is*, see the [README](README.md).

## Prerequisites

- **Rust 1.94+** (stable). Install via [rustup](https://rustup.rs/), then add the formatter and linter:
  ```bash
  rustup component add rustfmt clippy
  ```
- **Python 3.10+** with `pip`.
- **System packages:**
  - Debian / Ubuntu: `sudo apt install build-essential pkg-config libssl-dev`
  - macOS: `brew install openssl@3`

## Build

```bash
git clone https://github.com/beava-dev/beava.git
cd beava
cargo build --workspace
```

Install the Python SDK in editable mode:

```bash
cd python && pip install -e .
```

## Run the server

```bash
cargo run
# or, after `cargo build`:
./target/debug/beava
```

Defaults (override via `--http-addr` / `--tcp-addr` or env vars — see `cargo run -- --help`):

- HTTP / JSON listener: `127.0.0.1:8080`
- Binary-framed TCP listener: `127.0.0.1:8081`

## Run the tests

Run the same gates CI runs before opening a pull request.

**Rust:**

```bash
cargo test --workspace --features testing
```

**Python SDK:**

```bash
cd python
python -m pytest tests/v0 -q
```

The Python integration tests spawn the Beava binary, so run `cargo build` first.

## Lint and format

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

For the Python SDK:

```bash
ruff check python/beava
ruff format --check python/beava
mypy --strict python/beava
```

CI enforces all of the above. A pull request that fails any check will not be merged.

## Pull-request workflow

1. Fork [`beava-dev/beava`](https://github.com/beava-dev/beava) and create a feature branch off `main`.
2. Make your changes with tests.
3. Run the gates locally:
   ```bash
   cargo fmt --all --check \
     && cargo clippy --workspace --all-targets --all-features -- -D warnings \
     && cargo test --workspace --features testing
   ```
4. Use [conventional-commits](https://www.conventionalcommits.org/) commit subjects: `type(scope): subject` (`feat`, `fix`, `test`, `refactor`, `chore`, `docs`).
5. Open a pull request against `main`. Describe **what** changed and **why**.

## Reporting bugs

File issues at [`beava-dev/beava` GitHub Issues](https://github.com/beava-dev/beava/issues). Good reports include:

- Beava version (`beava --version`) and OS / platform.
- A minimal, runnable reproducer (curl commands, JSON payload, register definition).
- Expected vs actual behavior, with logs or stack traces if the server panics.

For feature requests, describe the use case first — the operator catalogue is intentionally narrow, so we tend to extend it through real workloads rather than speculative APIs.

## Reporting security vulnerabilities

**Do not file security issues on the public tracker.** See [SECURITY.md](SECURITY.md) for the disclosure process — in short: email `security@beava.dev` or use a GitHub private security advisory.

## License

Beava is licensed under [Apache 2.0](LICENSE). By submitting a pull request, you agree to license your contribution under the same terms.
