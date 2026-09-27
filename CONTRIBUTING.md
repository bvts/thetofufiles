# Contributing to TOFU

Thank you for your interest in contributing to TOFU!

## Development Setup

TOFU is built in pure Rust and requires Rust 1.75 or newer.

```bash
# Clone the repository
git clone https://github.com/tofu/tofu.git
cd tofu

# Build workspace
cargo build

# Run complete test suite
cargo test --workspace
```

## Architectural Guidelines

1. **Preserve Format Invariants**: Never modify binary offsets, magic identifiers, endianness, or checksum algorithms without a formal major version bump. See [docs/TOFU_FORMAT.md](docs/TOFU_FORMAT.md).
2. **Lossless Guarantee**: Every file stored in a `.tofu` container must be recoverable bit-for-bit identically. Tests must always assert `hash(original) == hash(extracted)`.
3. **Bounded Memory Usage**: Never load large archives or entire sample files into RAM at once. All I/O must stream through bounded buffers (default 64 KB).
4. **Security First**: All incoming paths must be sanitized through `Manifest::validate_path` and `sanitize_extract_path`.

## Submitting Pull Requests

1. Fork the repo and create your branch from `main`.
2. Ensure all tests pass (`cargo test --workspace`).
3. Add tests for any new features or bug fixes.
4. Format code using `cargo fmt`.
5. Open a pull request describing the changes and motivation.
