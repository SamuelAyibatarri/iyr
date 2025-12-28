use clap::Parser;
use notify_debouncer_full::{
    new_debouncer,
    notify::{EventKind, RecursiveMode},
};
use std::{fs::{self, File}, path::Path};
use std::sync::mpsc::channel;
use std::time::Duration;
use std::io::{Read, BufReader, self};
use crc32fast::Hasher;
use std::path::PathBuf;

// ----------------------
// CLI ARGS
// ----------------------
#[derive(Parser)]
struct Cli {
    path_a: String,
    path_b: String,

    #[arg(long)]
    overwrite: bool,
}

// ----------------------
// HELPER FUNCTIONS
// ----------------------

fn compute_hash(path: &Path) -> std::io::Result<u32> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Hasher::new();
    let mut buffer = [0; 8192];

    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 { break; }
        hasher.update(&buffer[..count]);    
    }
    Ok(hasher.finalize())
}

fn update_path(input: &str) -> PathBuf {
    let path = PathBuf::from(input);
    let parent = path.parent().unwrap_or(Path::new("."));
    
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("txt");

    let new_filename = format!("{}_backup.{}", stem, ext);
    parent.join(new_filename) 
}

// PHASE 1: Name & Extension Check
fn compare_stem_ext(path_a: &Path, path_b: &Path) -> bool {
    let name_a = path_a.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let name_b = path_b.file_name().and_then(|s| s.to_str()).unwrap_or("");

    // 1. Check for empty filenames
    if name_a.is_empty() || name_b.is_empty() {
        eprintln!("❌ Error: Invalid file paths provided.");
        return false;
    }

    // 2. Strict Name Equality (Case Insensitive)
    if !name_a.eq_ignore_ascii_case(name_b) {
        eprintln!("❌ Error: Files must have the same name and extension.");
        eprintln!("   File A: {}", name_a);
        eprintln!("   File B: {}", name_b);
        return false;
    }
    
    // 3. Extension Check
    if path_a.extension().is_none() || path_b.extension().is_none() {
         eprintln!("⚠️ Warning: One or both files have no extension.");
         // We allow this, but warn.
    }

    true
}

// PHASE 2: Magic Number & Content Check
fn is_valid_text_file(path: &Path) -> Result<bool, io::Error> {
    if !path.is_file() {
        return Ok(false);
    }

    let mut file = File::open(path)?;
    let mut buffer = [0u8; 1024];
    let bytes_read = file.read(&mut buffer)?;
    
    // Empty files are valid text files
    if bytes_read == 0 {
        return Ok(true);
    }

    let head = &buffer[..bytes_read];

    if infer::get(head).is_some() {
        return Ok(false);
    }

    match content_inspector::inspect(head) {
        content_inspector::ContentType::UTF_8 |
        content_inspector::ContentType::UTF_8_BOM => Ok(true),
        content_inspector::ContentType::BINARY => Ok(false),
        _ => Ok(false),
    }
}

// ----------------------
// MAIN APPLICATION
// ----------------------

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();
    
    let path_a = fs::canonicalize(&args.path_a).expect("File A must exist");
    let path_b = fs::canonicalize(&args.path_b).expect("File B must exist");

    let parent_a = path_a.parent().expect("File A has no parent directory");
    let parent_b = path_b.parent().expect("File B has no parent directory");

    println!("🔗 Linking: {:?} <==> {:?}", path_a, path_b);

    if !compare_stem_ext(&path_a, &path_b) {
        std::process::exit(1);
    }

    if !is_valid_text_file(&path_a)? || !is_valid_text_file(&path_b)? {
        eprintln!("❌ Error: One of the files is detected as Binary (Image/Video/Exec).");
        eprintln!("This tool only supports text-based files.");
        std::process::exit(1);
    }
    println!("✅ File Validation Passed (Text-only verified)");

    let mut hash_a = compute_hash(&path_a).unwrap_or(0);
    let mut hash_b = compute_hash(&path_b).unwrap_or(0);

    println!("📊 Initial Hashes -> A: {:x}, B: {:x}", hash_a, hash_b);

    if hash_a != hash_b {
        if !args.overwrite {
             eprintln!("❌ Files differ! Use '--overwrite' to sync them (creates backups).");
             std::process::exit(1);
        }

        let len_a = fs::metadata(&path_a).unwrap().len();
        let len_b = fs::metadata(&path_b).unwrap().len();

        if len_a > 0 && len_b > 0 {
            println!("⚠️ Conflict! Both files have content. Backing up and clearing...");
            
            let content_a = fs::read_to_string(&path_a).unwrap_or_default();
            let content_b = fs::read_to_string(&path_b).unwrap_or_default();

            // Create Backups
            fs::write(update_path(&args.path_a), &content_a)?;
            fs::write(update_path(&args.path_b), &content_b)?;

            // Decision: Sync A to B (Arbitrary choice for conflict resolution)
            println!("   Syncing A -> B");
            fs::write(&path_b, &content_a)?;
            
            hash_b = hash_a;
        } 
        else if len_a > 0 && len_b == 0 {
            println!("📥 B is empty. Syncing A -> B");
            let content_a = fs::read_to_string(&path_a).unwrap_or_default();
            fs::write(&path_b, &content_a)?; 
            hash_b = hash_a;
        } 
        else if len_b > 0 && len_a == 0 {
            println!("📥 A is empty. Syncing B -> A");
            let content_b = fs::read_to_string(&path_b).unwrap_or_default();
            fs::write(&path_a, &content_b)?;
            hash_a = hash_b;
        }
    } else {
        println!("✅ Files are identical.");
    }

    println!("👀 Starting watcher...");

    let (tx, rx) = channel();
    let mut debouncer = new_debouncer(Duration::from_millis(500), None, tx)?;

    debouncer.watch(parent_a, RecursiveMode::NonRecursive)?;
    if parent_a != parent_b {
        debouncer.watch(parent_b, RecursiveMode::NonRecursive)?;
    }

    // 6. Event Loop
    for result in rx {
        match result {
            Ok(events) => {
                let mut check_a = false;
                let mut check_b = false;

                for event in events {

                    if let EventKind::Modify(_) = event.kind {
                         for path in &event.paths {
                            if path == &path_a { check_a = true; }
                            if path == &path_b { check_b = true; }
                        }
                    }
                }

                if check_a {
                    if let Ok(new_hash) = compute_hash(&path_a) {
                        if new_hash != hash_a {
                            println!("🔄 File A changed ({:x}). Syncing to B...", new_hash);
                            hash_a = new_hash; 
                            if let Ok(content) = fs::read_to_string(&path_a) {
                                if let Err(e) = fs::write(&path_b, content) {
                                    eprintln!("Error writing B: {}", e);
                                } else {
                                    hash_b = new_hash; 
                                }
                            }
                        }
                    }
                }

                if check_b {
                    if let Ok(new_hash) = compute_hash(&path_b) {
                        if new_hash != hash_b {
                            println!("🔄 File B changed ({:x}). Syncing to A...", new_hash);
                            hash_b = new_hash;

                            if let Ok(content) = fs::read_to_string(&path_b) {
                                if let Err(e) = fs::write(&path_a, content) {
                                    eprintln!("Error writing A: {}", e);
                                } else {
                                    hash_a = new_hash;
                                }
                            }
                        }
                    }
                }
            },
            Err(e) => println!("Watch error: {:?}", e),
        }
    }

    Ok(())
}