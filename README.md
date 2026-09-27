# TOFU

A fast, lossless container format and toolchain for music-production presets and audio files.

`.tofu` files package presets and audio libraries into a single container using the custom TOFU format and Zstandard lossless compression.

> **The Core Guarantee:** Anything packed into a TOFU archive is recoverable byte-for-byte identically upon extraction, verified cryptographically via SHA-256.

```text
Files / Presets
       ↓
   tofu pack
       ↓
  Presets.tofu
       ↓
  tofu unpack
       ↓
 Original files (100% bit-exact)
```

TOFU is a general-purpose lossless binary container, designed with music production in mind:

* **Serum 2 presets** (`.fxp`, `.SerumPreset`)
* **Zenology & Vital presets**
* **Audio samples** (WAV, AIFF, FLAC)
* **Custom wavetables & noise files**
* **Impulse responses (IR)**
* **Soundbank collections & arbitrary files**

---

## Installation & PATH Setup

### Option 1: 1-Click Automated Setup (Windows)
Double-click `setup.bat` or `setup.exe`, or run from CMD/PowerShell:
```cmd
setup.bat
```
*(or run `setup.exe`)*

This automated installer:
1. Detects whether Rust is installed (and downloads/installs it via `rustup` if missing).
2. Builds and installs `tofu` globally into `%USERPROFILE%\.cargo\bin\tofu.exe`.
3. Ensures Cargo's bin directory is permanently in your Windows User PATH.
4. Generates an optimized standalone `target\release\tofu.exe`.

### Option 2: Install Globally with Cargo
If you already have Rust installed:
```powershell
cargo install --path crates/tofu-cli
```

### Option 3: Manual Build of the Windows Native Executable
```powershell
cargo build --release
```
The compiled executable is created at:
```text
target\release\tofu.exe
```

#### Add to Windows PATH (One-Time Setup):
To run `tofu` from any directory without typing its full path:

**In PowerShell (run once):**
```powershell
[Environment]::SetEnvironmentVariable(
    "Path",
    [Environment]::GetEnvironmentVariable("Path", "User") + ";C:\Path\To\tofu\target\release",
    "User"
)
```

Now you can open any CMD or PowerShell window and simply run:
```cmd
tofu
```

---

## Quick Start

### 1. Pack a Folder
Navigate into your presets folder:
```cmd
cd "C:\Downloads\MySoundbank"
tofu pack
```
TOFU automatically detects the current directory and creates:
```text
C:\Downloads\MySoundbank\MySoundbank.tofu
```

You can also specify explicit paths and metadata:
```cmd
tofu pack "C:\Downloads\MySoundbank" "C:\Downloads\SerumSounds.tofu" ^
    --name "Cyberpunk Soundbank" ^
    --plugin "Serum 2" ^
    --tags "Bass,Lead,WAV"
```

---

### 2. Unpack a Soundbank
To unpack an archive in the current directory:
```cmd
cd "C:\Downloads\MySoundbank"
tofu unpack
```
TOFU automatically finds the `.tofu` archive, restores all files with original timestamps, verifies SHA-256 hashes, and safely removes the archive.

#### ⚠️ Safe Deletion Behavior:
By default, `tofu unpack` cleans up the archive file after successful extraction and verification:

| Command | Behavior |
|---|---|
| `tofu unpack` | Extracts, verifies hashes, and prompts: `Delete archive? [Y/n]`. |
| `tofu unpack --yes` (`-y`) | Extracts, verifies, and deletes the archive without prompting. |
| `tofu unpack --keep` (`-k`) | Extracts and keeps the `.tofu` file on disk. |
| `tofu extract` | Alias for `tofu unpack --keep`. |

> **Safety Rule:** If extraction or checksum verification fails, TOFU **NEVER** deletes the `.tofu` file.

---

### 3. Inspect Contents Without Extracting
```cmd
tofu list "MySoundbank.tofu"
```
Displays all contained presets, sizes, compression methods, and truncated SHA-256 hashes without decompressing anything to disk.

---

### 4. Check Archive Information & Metadata
```cmd
tofu info "MySoundbank.tofu"
```
Shows the format version, target plugin, author, and compression ratios.

---

### 5. Verify Cryptographic Integrity
```cmd
tofu verify "MySoundbank.tofu"
```
Performs an in-memory audit of the header CRC-32, manifest SHA-256, and decompresses every file stream in memory to verify bit-for-bit lossless integrity without writing to disk:
```text
✓ TOFU archive is valid.
✓ 6 files verified bit-for-bit lossless.
✓ Integrity checks passed.
```

---

### 6. Version Info
```cmd
tofu version
```
```text
TOFU 1.0.0
TOFU Format v1.0
```

---

## Documentation

* [Format Specification (docs/TOFU_FORMAT.md)](docs/TOFU_FORMAT.md) — Authoritative binary layout, byte offsets, and header specification.
* [System Architecture (docs/ARCHITECTURE.md)](docs/ARCHITECTURE.md) — Engine subsystems, random-access seeking, and data flow.
* [CLI Manual (docs/CLI.md)](docs/CLI.md) — Complete argument table and exit codes.

---

## License

Licensed under the [MIT License](LICENSE).
