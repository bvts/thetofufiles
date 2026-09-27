# TOFU CLI Reference Manual

The `tofu` command-line utility provides commands to pack, inspect, verify, and extract `.tofu` preset containers.

---

## Command Overview

```text
tofu <COMMAND> [OPTIONS]
```

### Commands

| Command | Description |
|---|---|
| `pack` | Recursively pack files or a directory into a `.tofu` archive |
| `extract` | Safely extract a `.tofu` archive into a directory |
| `list` | List all contained files and directory entries without extracting |
| `info` | Display container header, metadata, compression stats, and integrity |
| `verify` | Cryptographically verify archive integrity and stored hashes |
| `help` | Print help information |

---

## 1. `tofu pack`

Pack files or directories into a `.tofu` container.

### Usage
```bash
tofu pack <INPUT> <OUTPUT> [OPTIONS]
```

### Arguments & Options
- `<INPUT>`: Directory or single file to pack.
- `<OUTPUT>`: Destination `.tofu` file path.
- `-l, --level <1-22>`: Zstandard compression level (default: `3`, set `0` for store mode).
- `-f, --force`: Overwrite existing output archive.
- `--name <STRING>`: Package title.
- `--author <STRING>`: Sound designer or author name.
- `--desc <STRING>`: Description of the preset pack.
- `--plugin <STRING>`: Target synthesizer or audio plugin (e.g. `"Serum 2"`, `"Vital"`, `"Kontakt"`).
- `--tags <STRING>`: Comma-separated tags (e.g. `"Bass,Acid,Techno"`).

### Example
```bash
tofu pack ./SerumBank ./SerumBank.tofu \
    --name "Cyberpunk Basses" \
    --author "SoundArchitect" \
    --plugin "Serum 2" \
    --tags "Bass,Cyberpunk,Industrial"
```

---

## 2. `tofu extract`

Extracts all files from a `.tofu` archive into a destination folder.

### Usage
```bash
tofu extract <ARCHIVE> <DESTINATION> [OPTIONS]
```

### Options
- `<ARCHIVE>`: Path to `.tofu` archive.
- `<DESTINATION>`: Folder where files should be extracted.
- `-f, --force`: Overwrite existing files in destination directory.
- `--no-mtime`: Do not restore original file modification timestamps.

### Example
```bash
tofu extract ./SerumBank.tofu ./ExtractedPresets/ --force
```

---

## 3. `tofu list`

Lists contained files, sizes, compression methods, and truncated SHA-256 digests.

### Usage
```bash
tofu list <ARCHIVE> [--json]
```

### Options
- `--json`: Output machine-readable JSON array of entries.

### Example
```bash
tofu list ./SerumBank.tofu
```

---

## 4. `tofu info`

Displays container version, metadata, file count, and compression ratios.

### Usage
```bash
tofu info <ARCHIVE> [--json]
```

### Options
- `--json`: Output machine-readable JSON object of metadata and archive statistics.

### Example
```bash
tofu info ./SerumBank.tofu
```

---

## 5. `tofu verify`

Performs in-memory cryptographic verification:
- Checks 80-byte header CRC-32.
- Authenticates Manifest SHA-256 table.
- Authenticates Metadata SHA-256 (if present).
- Stream-decompresses every file into an in-memory hashing sink and checks SHA-256.

### Usage
```bash
tofu verify <ARCHIVE> [--json]
```

### Options
- `--json`: Output machine-readable JSON verification status.

### Example
```bash
tofu verify ./SerumBank.tofu
```

---

## Exit Codes

The `tofu` CLI returns standard, descriptive process exit codes:

| Exit Code | Meaning |
|---|---|
| `0` | **Success**: Operation completed with 100% cryptographic verification. |
| `1` | **Integrity Failure**: Checksum or SHA-256 mismatch detected. |
| `2` | **Corrupted / Invalid Archive**: Magic mismatch, unsupported version, or truncation. |
| `3` | **Security Violation**: Path traversal attempt (`../`), absolute path, or case collision. |
| `4` | **I/O Error**: Missing input file, read/write failure, or destination exists without `--force`. |
| `5` | **General Error**: Unexpected error. |
