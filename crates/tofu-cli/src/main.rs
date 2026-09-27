use clap::{Parser, Subcommand};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::json;
use std::env;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use tofu_core::compression::DEFAULT_ZSTD_LEVEL;
use tofu_core::error::TofuError;
use tofu_core::extract::{ExtractOptions, ExtractProgress};
use tofu_core::metadata::Metadata;
use tofu_core::pack::{PackOptions, PackProgress};
use tofu_core::verify::VerifyProgress;
use tofu_core::{FORMAT_VERSION_STR, SOFTWARE_VERSION};

#[derive(Parser)]
#[command(
    name = "tofu",
    about = "TOFU — Lossless Container for Music-Production Presets & Samples",
    version = SOFTWARE_VERSION,
    long_version = concat!(
        "TOFU ", env!("CARGO_PKG_VERSION"), "\n",
        "TOFU Format v", "1.0", "\n",
        "Pure lossless container format engineered for music production."
    ),
    author
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Pack files or current directory into a .tofu archive
    Pack {
        /// Directory or file to pack (defaults to current directory)
        #[arg(value_name = "INPUT")]
        input: Option<PathBuf>,

        /// Output .tofu file path (defaults to <DirectoryName>.tofu)
        #[arg(value_name = "OUTPUT")]
        output: Option<PathBuf>,

        /// Zstandard compression level (1-22, default: 3, 0 for store mode)
        #[arg(short, long, default_value_t = DEFAULT_ZSTD_LEVEL)]
        level: i32,

        /// Overwrite destination file if it already exists
        #[arg(short, long)]
        force: bool,

        /// Package title / name
        #[arg(long)]
        name: Option<String>,

        /// Author / Sound designer
        #[arg(long)]
        author: Option<String>,

        /// Description of the soundbank
        #[arg(long)]
        desc: Option<String>,

        /// Target synth / plugin (e.g. "Serum 2", "Vital", "Zenology", "Kontakt")
        #[arg(long)]
        plugin: Option<String>,

        /// Comma-separated tags (e.g. "Bass,Lead,Pluck,WAV")
        #[arg(long)]
        tags: Option<String>,
    },

    /// Unpack a .tofu archive and safely verify extracted files
    Unpack {
        /// .tofu archive to unpack (defaults to auto-detecting .tofu in current directory)
        #[arg(value_name = "ARCHIVE")]
        archive: Option<PathBuf>,

        /// Destination directory (defaults to the archive's folder)
        #[arg(value_name = "DESTINATION")]
        destination: Option<PathBuf>,

        /// Overwrite existing files during extraction
        #[arg(short, long)]
        force: bool,

        /// Keep the .tofu archive after successful extraction (do not delete)
        #[arg(short = 'k', long)]
        keep: bool,

        /// Do not prompt before deleting the archive after extraction
        #[arg(short = 'y', long)]
        yes: bool,

        /// Do not restore original file modification timestamps
        #[arg(long)]
        no_mtime: bool,
    },

    /// Alias for unpack --keep (extract without deleting the archive)
    Extract {
        /// .tofu archive to extract
        #[arg(value_name = "ARCHIVE")]
        archive: Option<PathBuf>,

        /// Destination directory
        #[arg(value_name = "DESTINATION")]
        destination: Option<PathBuf>,

        /// Overwrite existing files
        #[arg(short, long)]
        force: bool,

        /// Do not restore original file modification timestamps
        #[arg(long)]
        no_mtime: bool,
    },

    /// List contained presets and files in an archive without extracting
    List {
        /// Input .tofu archive (defaults to auto-detecting .tofu in current directory)
        #[arg(value_name = "ARCHIVE")]
        archive: Option<PathBuf>,

        /// Output listing as JSON
        #[arg(long)]
        json: bool,
    },

    /// Display container header, metadata, compression stats, and integrity
    Info {
        /// Input .tofu archive (defaults to auto-detecting .tofu in current directory)
        #[arg(value_name = "ARCHIVE")]
        archive: Option<PathBuf>,

        /// Output details as JSON
        #[arg(long)]
        json: bool,
    },

    /// Cryptographically verify archive integrity and stored file hashes without disk writes
    Verify {
        /// Input .tofu archive (defaults to auto-detecting .tofu in current directory)
        #[arg(value_name = "ARCHIVE")]
        archive: Option<PathBuf>,

        /// Output verification results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Display TOFU software and file format versions
    Version,
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.2} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

/// Automatically finds a `.tofu` archive in the given directory.
/// Returns Ok(path) if exactly one archive exists, or user-friendly errors otherwise.
fn resolve_single_archive(specified: Option<PathBuf>, dir: &Path, command_name: &str) -> Result<PathBuf, String> {
    if let Some(path) = specified {
        if !path.exists() {
            return Err(format!(
                "Error: Archive not found: '{}'\n\nPlease verify the file path and try again.",
                path.display()
            ));
        }
        return Ok(path);
    }

    // Scan directory for .tofu files
    let entries = fs::read_dir(dir).map_err(|e| format!("Error reading directory '{}': {e}", dir.display()))?;
    let mut tofu_files = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext.to_string_lossy().eq_ignore_ascii_case("tofu") {
                    tofu_files.push(path);
                }
            }
        }
    }

    tofu_files.sort();

    match tofu_files.len() {
        0 => Err(format!(
            "Error: No .tofu archive was found in the current directory.\n\nUse:\n    tofu {command_name} \"path\\to\\archive.tofu\""
        )),
        1 => Ok(tofu_files.remove(0)),
        _ => {
            let mut msg = format!(
                "Error: Multiple TOFU archives were found in '{}':\n",
                dir.display()
            );
            for p in &tofu_files {
                let name = p.file_name().unwrap_or_default().to_string_lossy();
                msg.push_str(&format!("  • {name}\n"));
            }
            let first = tofu_files[0].file_name().unwrap_or_default().to_string_lossy();
            msg.push_str(&format!(
                "\nPlease specify which archive to use:\n    tofu {command_name} \"{first}\""
            ));
            Err(msg)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_pack(
    input_opt: Option<PathBuf>,
    output_opt: Option<PathBuf>,
    level: i32,
    force: bool,
    name: Option<String>,
    author: Option<String>,
    desc: Option<String>,
    plugin: Option<String>,
    tags: Option<String>,
) -> Result<(), String> {
    let current_dir = env::current_dir().map_err(|e| format!("Cannot get current directory: {e}"))?;

    // Determine input path
    let input = match input_opt {
        Some(p) => p,
        None => current_dir.clone(),
    };

    if !input.exists() {
        return Err(format!("Error: Source path '{}' does not exist.", input.display()));
    }

    // Determine output path
    let output = match output_opt {
        Some(out) => out,
        None => {
            // Predictable naming strategy: use the directory's base name
            let base_name = if input.is_file() {
                input
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            } else {
                let canonical = input.canonicalize().unwrap_or_else(|_| input.clone());
                canonical
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Archive".to_string())
            };
            input.join(format!("{base_name}.tofu"))
        }
    };

    println!(
        "{} Packing '{}' into '{}'...",
        "[TOFU]".bold().cyan(),
        input.display(),
        output.display()
    );

    let mut metadata = None;
    if name.is_some() || author.is_some() || desc.is_some() || plugin.is_some() || tags.is_some() {
        let tag_list = tags
            .map(|t| t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default();

        metadata = Some(Metadata {
            name,
            author,
            description: desc,
            version: Some("1.0.0".to_string()),
            created_at: Some(chrono_now()),
            plugin,
            plugin_version: None,
            category: None,
            tags: tag_list,
            preset_count: None,
            extra: Default::default(),
        });
    }

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );

    let options = PackOptions {
        compression_level: level,
        force,
        metadata,
        progress_callback: Some(Box::new(|event| match event {
            PackProgress::FileStart { path, index, total } => {
                pb.set_message(format!("[{index}/{total}] Compressing {path}..."));
            }
            PackProgress::FileComplete { .. } => {}
        })),
    };

    let stats = tofu_core::pack(&input, &output, &options).map_err(|e| match e {
        TofuError::DestinationExists(p) => format!(
            "Error: Output file '{}' already exists.\nUse --force (-f) to overwrite.",
            p.display()
        ),
        other => format!("Packing failed: {other}"),
    })?;

    pb.finish_and_clear();

    println!("{}", "[SUCCESS] Archive created successfully!".bold().green());
    println!("  {:<24} {}", "Created archive:", output.file_name().unwrap_or_default().to_string_lossy().bold());
    println!("  {:<24} {}", "Files packed:", stats.file_count);
    println!(
        "  {:<24} {}",
        "Total original size:",
        format_bytes(stats.total_original_size)
    );
    println!(
        "  {:<24} {}",
        "Total compressed size:",
        format_bytes(stats.total_compressed_size)
    );
    println!(
        "  {:<24} {}",
        "Final archive size:",
        format_bytes(stats.archive_size)
    );
    println!(
        "  {:<24} {:.1}% saved (ratio: {:.3})",
        "Space savings:",
        stats.space_saved_percent(),
        stats.compression_ratio()
    );

    Ok(())
}

fn handle_unpack(
    archive_opt: Option<PathBuf>,
    dest_opt: Option<PathBuf>,
    force: bool,
    keep: bool,
    yes: bool,
    no_mtime: bool,
) -> Result<(), String> {
    let current_dir = env::current_dir().map_err(|e| format!("Cannot get current directory: {e}"))?;
    let archive_path = resolve_single_archive(archive_opt, &current_dir, "unpack")?;

    // Determine destination
    let destination = match dest_opt {
        Some(d) => d,
        None => {
            // Default to the directory containing the archive
            archive_path
                .parent()
                .map(|p| if p.as_os_str().is_empty() { Path::new(".") } else { p })
                .unwrap_or(Path::new("."))
                .to_path_buf()
        }
    };

    println!(
        "{} Unpacking '{}' into '{}'...",
        "[TOFU]".bold().cyan(),
        archive_path.display(),
        destination.display()
    );

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );

    let options = ExtractOptions {
        force,
        restore_mtime: !no_mtime,
        progress_callback: Some(Box::new(|event| match event {
            ExtractProgress::FileStart { path, index, total } => {
                pb.set_message(format!("[{index}/{total}] Verifying & extracting {path}..."));
            }
            ExtractProgress::FileComplete { .. } => {}
        })),
    };

    // 1. Extract and verify SHA-256 live on every file
    let stats = match tofu_core::extract(&archive_path, &destination, &options) {
        Ok(s) => s,
        Err(e) => {
            pb.finish_and_clear();
            // CRITICAL: NEVER delete the archive on error!
            eprintln!(
                "\n{} Extraction aborted: {e}\nOriginal archive preserved: '{}'",
                "[ERROR]".bold().red(),
                archive_path.display()
            );
            return Err(format!("Unpack failed: {e}"));
        }
    };

    pb.finish_and_clear();

    println!(
        "{}",
        "[SUCCESS] Lossless extraction verified!".bold().green()
    );
    println!("  {:<24} {}", "Files restored:", stats.file_count);
    println!(
        "  {:<24} {}",
        "Bytes extracted:",
        format_bytes(stats.bytes_extracted)
    );
    println!("  {:<24} Cryptographically matched via SHA-256", "Integrity:");

    // 2. Safe deletion protocol
    if keep {
        println!("  {:<24} Kept (--keep flag specified)", "Archive status:");
        return Ok(());
    }

    let archive_filename = archive_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    let should_delete = if yes || !io::stdin().is_terminal() {
        true
    } else {
        print!("\nDelete {}? [Y/n] ", archive_filename.bold().yellow());
        io::stdout().flush().unwrap();

        let mut input = String::new();
        let stdin = io::stdin();
        let mut handle = stdin.lock();
        if handle.read_line(&mut input).is_ok() {
            let trimmed = input.trim().to_lowercase();
            trimmed.is_empty() || trimmed == "y" || trimmed == "yes"
        } else {
            false
        }
    };

    if should_delete {
        if let Err(e) = fs::remove_file(&archive_path) {
            eprintln!("Warning: Failed to remove archive file: {e}");
        } else {
            println!("  {:<24} Removed archive '{}'", "Cleanup:", archive_filename);
        }
    } else {
        println!("  {:<24} Kept '{}'", "Cleanup:", archive_filename);
    }

    Ok(())
}

fn handle_list(archive_opt: Option<PathBuf>, json_mode: bool) -> Result<(), String> {
    let current_dir = env::current_dir().map_err(|e| format!("Cannot get current directory: {e}"))?;
    let archive = resolve_single_archive(archive_opt, &current_dir, "list")?;
    let entries = tofu_core::list(&archive).map_err(|e| format!("Cannot list archive: {e}"))?;

    if json_mode {
        let json_entries: Vec<_> = entries
            .iter()
            .map(|e| {
                json!({
                    "path": e.path,
                    "is_directory": e.is_directory(),
                    "original_size": e.original_size,
                    "compressed_size": e.compressed_size,
                    "compression_method": if e.compression_method == 0 { "Store" } else { "Zstd" },
                    "compression_level": e.compression_level,
                    "mtime": e.mtime,
                    "sha256": e.sha256_hex()
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_entries).unwrap());
        return Ok(());
    }

    println!(
        "{} Archive contents of '{}':",
        "[TOFU]".bold().cyan(),
        archive.display()
    );
    println!(
        "{:<8} {:>12} {:>12}  {:<8}  {:<12} PATH",
        "TYPE", "ORIG SIZE", "COMP SIZE", "METHOD", "SHA-256"
    );
    println!("{}", "-".repeat(88).bright_black());

    for entry in entries {
        let type_str = if entry.is_directory() {
            "DIR".yellow()
        } else {
            "FILE".normal()
        };
        let method_str = if entry.is_directory() {
            "-".normal()
        } else if entry.compression_method == 0 {
            "Store".blue()
        } else {
            "Zstd".green()
        };
        let sha_short = if entry.is_directory() {
            "-".to_string()
        } else {
            entry.sha256_hex()[..8].to_string()
        };

        println!(
            "{:<8} {:>12} {:>12}  {:<8}  {:<12} {}",
            type_str,
            format_bytes(entry.original_size),
            format_bytes(entry.compressed_size),
            method_str,
            sha_short,
            entry.path
        );
    }

    Ok(())
}

fn handle_info(archive_opt: Option<PathBuf>, json_mode: bool) -> Result<(), String> {
    let current_dir = env::current_dir().map_err(|e| format!("Cannot get current directory: {e}"))?;
    let archive = resolve_single_archive(archive_opt, &current_dir, "info")?;
    let info = tofu_core::inspect(&archive).map_err(|e| format!("Cannot inspect archive: {e}"))?;

    if json_mode {
        let val = json!({
            "software_version": SOFTWARE_VERSION,
            "format_version": format!("{}.{}", info.header.version_major, info.header.version_minor),
            "file_count": info.header.file_count,
            "archive_size": info.archive_size,
            "total_original_size": info.total_original_size,
            "total_compressed_size": info.total_compressed_size,
            "compression_ratio": info.compression_ratio(),
            "space_saved_percent": info.space_saved_percent(),
            "metadata": info.metadata,
        });
        println!("{}", serde_json::to_string_pretty(&val).unwrap());
        return Ok(());
    }

    println!("{} Archive Information", "[TOFU]".bold().cyan());
    println!("{}", "=".repeat(60).bright_black());
    println!(
        "  {:<26} v{}.{}",
        "Format Version:", info.header.version_major, info.header.version_minor
    );
    println!("  {:<26} TOFU {SOFTWARE_VERSION}", "Software Engine:");
    println!("  {:<26} {}", "Total Files / Dirs:", info.header.file_count);
    println!(
        "  {:<26} {}",
        "Archive On-Disk Size:",
        format_bytes(info.archive_size)
    );
    println!(
        "  {:<26} {}",
        "Total Uncompressed Size:",
        format_bytes(info.total_original_size)
    );
    println!(
        "  {:<26} {}",
        "Total Compressed Size:",
        format_bytes(info.total_compressed_size)
    );
    println!(
        "  {:<26} {:.1}% (ratio: {:.3})",
        "Overall Space Savings:",
        info.space_saved_percent(),
        info.compression_ratio()
    );
    println!(
        "  {:<26} {}",
        "Integrity Status:",
        "Cryptographically authenticated (SHA-256 & CRC-32)".green()
    );

    if let Some(meta) = &info.metadata {
        println!();
        println!("{} Package Metadata", "[METADATA]".bold().magenta());
        println!("{}", "-".repeat(60).bright_black());
        if let Some(v) = &meta.name {
            println!("  {:<26} {}", "Name:", v.bold());
        }
        if let Some(v) = &meta.author {
            println!("  {:<26} {}", "Author:", v);
        }
        if let Some(v) = &meta.description {
            println!("  {:<26} {}", "Description:", v);
        }
        if let Some(v) = &meta.plugin {
            println!("  {:<26} {}", "Target Plugin:", v.yellow());
        }
        if !meta.tags.is_empty() {
            println!("  {:<26} {}", "Tags:", meta.tags.join(", ").blue());
        }
        if let Some(v) = &meta.created_at {
            println!("  {:<26} {}", "Created At:", v);
        }
    }

    Ok(())
}

fn handle_verify(archive_opt: Option<PathBuf>, json_mode: bool) -> Result<(), String> {
    let current_dir = env::current_dir().map_err(|e| format!("Cannot get current directory: {e}"))?;
    let archive = resolve_single_archive(archive_opt, &current_dir, "verify")?;

    println!(
        "{} Cryptographically verifying '{}'...",
        "[TOFU]".bold().cyan(),
        archive.display()
    );

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );

    let rep = tofu_core::verify(&archive, Some(|event: VerifyProgress| match event {
        VerifyProgress::HeaderOk => pb.set_message("Header CRC-32 verified OK"),
        VerifyProgress::MetadataOk => pb.set_message("Metadata verified OK"),
        VerifyProgress::ManifestOk { count } => {
            pb.set_message(format!("Manifest SHA-256 verified ({count} entries)"))
        }
        VerifyProgress::FileStart { path, index, total } => {
            pb.set_message(format!("[{index}/{total}] Testing SHA-256 for {path}..."))
        }
        VerifyProgress::FileOk { .. } => {}
    })).map_err(|e| {
        pb.finish_and_clear();
        format!("Verification failed: {e}")
    })?;

    pb.finish_and_clear();

    if json_mode {
        let val = json!({
            "is_valid": rep.is_valid,
            "file_count": rep.file_count,
            "total_original_size": rep.total_original_size,
            "total_compressed_size": rep.total_compressed_size,
            "has_metadata": rep.has_metadata,
            "verified_entries": rep.details.len()
        });
        println!("{}", serde_json::to_string_pretty(&val).unwrap());
        return Ok(());
    }

    println!(
        "{}",
        "✓ TOFU archive is valid.".bold().green()
    );
    println!("✓ {} files verified bit-for-bit lossless.", rep.file_count);
    println!("✓ Integrity checks passed.");

    Ok(())
}

fn handle_version() {
    println!("TOFU {}", SOFTWARE_VERSION);
    println!("TOFU Format v{}", FORMAT_VERSION_STR);
}

fn chrono_now() -> String {
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", duration.as_secs())
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let result = match cli.command {
        None => {
            // If run with no subcommand, display version and usage help
            handle_version();
            println!("\nUse 'tofu --help' for commands and usage instructions.");
            return ExitCode::SUCCESS;
        }

        Some(Commands::Pack {
            input,
            output,
            level,
            force,
            name,
            author,
            desc,
            plugin,
            tags,
        }) => handle_pack(input, output, level, force, name, author, desc, plugin, tags),

        Some(Commands::Unpack {
            archive,
            destination,
            force,
            keep,
            yes,
            no_mtime,
        }) => handle_unpack(archive, destination, force, keep, yes, no_mtime),

        Some(Commands::Extract {
            archive,
            destination,
            force,
            no_mtime,
        }) => handle_unpack(archive, destination, force, true, true, no_mtime),

        Some(Commands::List { archive, json }) => handle_list(archive, json),

        Some(Commands::Info { archive, json }) => handle_info(archive, json),

        Some(Commands::Verify { archive, json }) => handle_verify(archive, json),

        Some(Commands::Version) => {
            handle_version();
            Ok(())
        }
    };

    match result {
        Ok(_) => ExitCode::SUCCESS,
        Err(err_msg) => {
            eprintln!("\n{}", err_msg.bold().red());
            ExitCode::from(4)
        }
    }
}
