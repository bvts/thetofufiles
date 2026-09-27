use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use tempfile::tempdir;
use tofu_core::hashing::hash_bytes;
use tofu_core::metadata::Metadata;
use tofu_core::pack::{pack, PackOptions};
use tofu_core::extract::{extract, ExtractOptions};
use tofu_core::verify::verify;
use tofu_core::list;

/// Helper: calculates SHA-256 of a file on disk.
fn compute_file_sha256(path: &Path) -> [u8; 32] {
    let mut file = File::open(path).expect("Failed to open file for hashing");
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).expect("Failed to read file");
    hash_bytes(&buffer)
}

/// Helper: generates a valid 16-bit 44.1kHz stereo PCM WAV file.
fn generate_synthetic_wav(sample_rate: u32, channels: u16, num_samples: usize) -> Vec<u8> {
    let bytes_per_sample = 2; // 16-bit
    let block_align = channels * bytes_per_sample;
    let byte_rate = sample_rate * block_align as u32;
    let data_size = num_samples * block_align as usize;
    let chunk_size = 36 + data_size;

    let mut wav = Vec::with_capacity(44 + data_size);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(chunk_size as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // Subchunk1Size
    wav.extend_from_slice(&1u16.to_le_bytes());  // AudioFormat (PCM = 1)
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes()); // BitsPerSample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_size as u32).to_le_bytes());

    // Generate sinusoidal PCM audio data
    for i in 0..num_samples {
        let t = i as f32 / sample_rate as f32;
        let sample_val = (t * 440.0 * 2.0 * std::f32::consts::PI).sin();
        let sample_i16 = (sample_val * 32767.0) as i16;
        for _ in 0..channels {
            wav.extend_from_slice(&sample_i16.to_le_bytes());
        }
    }

    wav
}

#[test]
fn test_single_tiny_file_roundtrip() {
    let dir = tempdir().unwrap();
    let src_file = dir.path().join("solo_preset.fxp");
    let content = b"PresetName=UltraBass;Cutoff=64;Resonance=32;";
    fs::write(&src_file, content).unwrap();

    let archive = dir.path().join("single.tofu");
    let extracted_dir = dir.path().join("extracted");

    let opts = PackOptions::default();
    pack(&src_file, &archive, &opts).expect("Failed to pack single file");

    let ext_opts = ExtractOptions::default();
    extract(&archive, &extracted_dir, &ext_opts).expect("Failed to extract");

    let restored = extracted_dir.join("solo_preset.fxp");
    assert!(restored.exists());
    let restored_bytes = fs::read(&restored).unwrap();
    assert_eq!(restored_bytes, content);
}

#[test]
fn test_complex_preset_pack_roundtrip() {
    let dir = tempdir().unwrap();
    let src_dir = dir.path().join("MyPresetPack");
    fs::create_dir_all(src_dir.join("Serum/Bass")).unwrap();
    fs::create_dir_all(src_dir.join("Serum/Leads")).unwrap();
    fs::create_dir_all(src_dir.join("Samples/Kicks")).unwrap();
    fs::create_dir_all(src_dir.join("Documentation")).unwrap();
    fs::create_dir_all(src_dir.join("EmptyFolder")).unwrap();

    // 1. Synth Presets
    fs::write(src_dir.join("Serum/Bass/Sub 01.fxp"), b"Serum Binary Preset Chunk V1 0001").unwrap();
    fs::write(src_dir.join("Serum/Bass/Reese 02.fxp"), b"Serum Binary Preset Chunk V1 0002").unwrap();
    fs::write(src_dir.join("Serum/Leads/Pluck 01.fxp"), b"Serum Binary Preset Chunk V1 0003").unwrap();

    // 2. Audio WAV files (valid RIFF format)
    let kick_wav = generate_synthetic_wav(44100, 2, 44100 / 2); // 0.5s stereo
    fs::write(src_dir.join("Samples/Kicks/Heavy_Kick.wav"), &kick_wav).unwrap();

    // 3. Documentation
    fs::write(src_dir.join("Documentation/README.txt"), b"Enjoy these royalty-free presets!").unwrap();

    // 4. Record original hashes
    let mut original_hashes = HashMap::new();
    for entry in walkdir::WalkDir::new(&src_dir) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            let rel = entry.path().strip_prefix(&src_dir).unwrap().to_string_lossy().replace('\\', "/");
            let hash = compute_file_sha256(entry.path());
            original_hashes.insert(rel, hash);
        }
    }
    assert_eq!(original_hashes.len(), 5);

    // 5. Pack with Metadata
    let archive_path = dir.path().join("pack.tofu");
    let mut meta = Metadata::new();
    meta.name = Some("Future Rave Sounds".to_string());
    meta.author = Some("SynthMaster".to_string());
    meta.plugin = Some("Serum 2".to_string());
    meta.tags = vec!["Rave".to_string(), "Bass".to_string()];

    let pack_opts = PackOptions {
        compression_level: 3,
        force: false,
        metadata: Some(meta),
        progress_callback: None,
    };

    let pack_stats = pack(&src_dir, &archive_path, &pack_opts).expect("Packing failed");
    assert_eq!(pack_stats.file_count, 12); // 5 files + 7 directory entries

    // 6. Verify in-memory (0 disk writes)
    let rep = verify(&archive_path, None::<fn(tofu_core::verify::VerifyProgress)>).expect("Verification failed");
    assert!(rep.is_valid);
    assert_eq!(rep.file_count, 12);

    // 7. Extract into new directory
    let ext_dir = dir.path().join("extracted_pack");
    let ext_opts = ExtractOptions::default();
    let ext_stats = extract(&archive_path, &ext_dir, &ext_opts).expect("Extraction failed");
    assert_eq!(ext_stats.file_count, 12);

    // 8. Assert EVERY file is byte-for-byte identical via SHA-256
    for (rel_path, orig_hash) in original_hashes {
        let extracted_file = ext_dir.join(&rel_path);
        assert!(extracted_file.exists(), "Extracted file missing: {}", rel_path);
        let extracted_hash = compute_file_sha256(&extracted_file);
        assert_eq!(
            orig_hash, extracted_hash,
            "SHA-256 hash mismatch for file '{}'! Original: {}, Extracted: {}",
            rel_path,
            hex::encode(orig_hash),
            hex::encode(extracted_hash)
        );
    }

    // 9. Assert empty directory was created
    assert!(ext_dir.join("EmptyFolder").is_dir());
}

#[test]
fn test_unicode_and_spaces_roundtrip() {
    let dir = tempdir().unwrap();
    let src_dir = dir.path().join("Unicode Pack");
    fs::create_dir_all(&src_dir).unwrap();

    let files = vec![
        ("Preset #01 - Säntis リード 🎵.fxp", b"Synthesizer preset data 1".to_vec()),
        ("Deep Bass - 808 & 909 (Remix) [Special].vital", b"Vital synth JSON { osc: 1 }".to_vec()),
        ("日本語のプリセット.json", b"{ \"synth\": \"Zenology\", \"patch\": 102 }".to_vec()),
        ("Accents - Café & Crème Brûlée.wav", generate_synthetic_wav(44100, 1, 1000)),
    ];

    for (name, content) in &files {
        fs::write(src_dir.join(name), content).unwrap();
    }

    let archive = dir.path().join("unicode.tofu");
    pack(&src_dir, &archive, &PackOptions::default()).unwrap();

    let entries = list(&archive).unwrap();
    assert_eq!(entries.len(), 4);

    let ext_dir = dir.path().join("extracted_unicode");
    extract(&archive, &ext_dir, &ExtractOptions::default()).unwrap();

    for (name, expected_content) in files {
        let p = ext_dir.join(name);
        assert!(p.exists(), "Missing file with special characters: {}", name);
        let actual = fs::read(&p).unwrap();
        assert_eq!(actual, expected_content, "Content mismatch for {}", name);
    }
}

#[test]
fn test_large_sample_file_streaming_roundtrip() {
    let dir = tempdir().unwrap();
    let src_file = dir.path().join("large_multitrack.wav");

    // 4 MB synthetic audio sample stream
    let big_wav = generate_synthetic_wav(48000, 2, 48000 * 20); // ~3.8 MB
    fs::write(&src_file, &big_wav).unwrap();
    let original_hash = compute_file_sha256(&src_file);

    let archive = dir.path().join("large.tofu");
    pack(&src_file, &archive, &PackOptions::default()).unwrap();

    let rep = verify(&archive, None::<fn(tofu_core::verify::VerifyProgress)>).unwrap();
    assert!(rep.is_valid);

    let ext_dir = dir.path().join("extracted_large");
    extract(&archive, &ext_dir, &ExtractOptions::default()).unwrap();

    let restored = ext_dir.join("large_multitrack.wav");
    let restored_hash = compute_file_sha256(&restored);
    assert_eq!(original_hash, restored_hash);
}

#[test]
fn test_empty_archive_roundtrip() {
    let dir = tempdir().unwrap();
    let empty_src = dir.path().join("empty_folder");
    fs::create_dir_all(&empty_src).unwrap();

    let archive = dir.path().join("empty.tofu");
    let stats = pack(&empty_src, &archive, &PackOptions::default()).expect("Packing empty directory failed");
    assert_eq!(stats.file_count, 0);

    let rep = verify(&archive, None::<fn(tofu_core::verify::VerifyProgress)>).expect("Verifying empty archive failed");
    assert!(rep.is_valid);
    assert_eq!(rep.file_count, 0);

    let ext_dir = dir.path().join("extracted_empty");
    let ext_stats = extract(&archive, &ext_dir, &ExtractOptions::default()).expect("Extracting empty archive failed");
    assert_eq!(ext_stats.file_count, 0);
}

#[test]
fn test_zero_byte_file_roundtrip() {
    let dir = tempdir().unwrap();
    let src_dir = dir.path().join("zero_byte_pack");
    fs::create_dir_all(&src_dir).unwrap();

    // 0-byte sentinel files (e.g. .license, .empty)
    fs::write(src_dir.join(".license"), b"").unwrap();
    fs::write(src_dir.join("empty_marker.fxp"), b"").unwrap();

    let archive = dir.path().join("zero_byte.tofu");
    let stats = pack(&src_dir, &archive, &PackOptions::default()).unwrap();
    assert_eq!(stats.file_count, 2);

    let rep = verify(&archive, None::<fn(tofu_core::verify::VerifyProgress)>).unwrap();
    assert!(rep.is_valid);

    let ext_dir = dir.path().join("extracted_zero");
    extract(&archive, &ext_dir, &ExtractOptions::default()).unwrap();

    assert_eq!(fs::read(ext_dir.join(".license")).unwrap().len(), 0);
    assert_eq!(fs::read(ext_dir.join("empty_marker.fxp")).unwrap().len(), 0);
}

