# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-09-27

### Added
- **TOFU v1.0 Binary Specification**: Fixed 80-byte header, Little-Endian determinism, CRC-32 header integrity, and $O(1)$ random-access seeking.
- **Lossless Zstandard Compression**: High-efficiency compression with configurable levels (1–22) and Auto-Store fallback for tiny presets.
- **Cryptographic Integrity**: Streaming SHA-256 for all uncompressed payloads and compressed chunk CRC-32.
- **Extensible Package Metadata**: Support for package title, sound designer, target synth plugin, categories, and tags.
- **Timestamp Preservation**: Preserves original file modification dates (`mtime`) for DAW preset browsers.
- **Smart CLI Tool**:
  - `tofu pack`: Automatic current-directory packaging with auto-naming (`<Folder>.tofu`).
  - `tofu unpack`: Single-archive auto-detection, default in-place extraction, and safe deletion protocol (`--keep`, `--yes`).
  - `tofu list`: Zero-extraction archive content listing.
  - `tofu info`: Metadata, versioning, and space savings reporting.
  - `tofu verify`: In-memory cryptographic proof of archive and file integrity.
  - `tofu version`: Clear separation between software version (`1.0.0`) and format version (`v1.0`).
- **Path Sanitization & Security**: Strict traversal defenses (`..`, absolute paths, Windows drive letters) and case-insensitive collision guards.
- **Automated Test Suite**: 31 unit, corruption, security, roundtrip, and CLI integration tests.
