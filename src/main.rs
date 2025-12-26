use clap::Parser;
use notify_debouncer_full::{
    new_debouncer,
    notify::{EventKind, RecursiveMode, Watcher},
};
use std::{fs::{self, File}, path::Path};
use std::sync::mpsc::channel;
use std::time::Duration;
use std::io::{Read, BufReader};
use crc32fast::Hasher;
use std::path::PathBuf;

#[derive(Parser)]
struct Cli {
    path_a: String,
    path_b: String,

    #[arg(long)]
    overwrite: bool,
}

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Cli::parse();
    
    let path_a = fs::canonicalize(&args.path_a).expect("File a must exist");
    let parent_a = path_a.parent().expect("File a has no parent directory");

    let path_b = fs::canonicalize(&args.path_b).expect("File b must exist");
    let parent_b = path_b.parent().expect("File b has no parent directory");

    println!("Linking: \n {:?}\n <==>\n {:?}", path_a, path_b);

    let mut hash_a = compute_hash(&path_a).unwrap_or(0);
    let mut hash_b = compute_hash(&path_b).unwrap_or(0);

    println!("Initial Hashes -> A: {:x}, B: {:x}", hash_a, hash_b);

    if hash_a == hash_b {
        println!("✅ Files are identical. Starting watcher...");
    } else {
        if !args.overwrite {
             panic!("❌ Files differ! Use '--overwrite' to sync them (creating backups) or fix manually.");
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

            // Clear Originals
            fs::write(&path_a, "")?; 
            fs::write(&path_b, "")?;
            
            // Update in-memory hashes
            hash_a = 0; 
            hash_b = 0;
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
    }

    let (tx, rx) = channel();
    let mut debouncer = new_debouncer(Duration::from_millis(500), None, tx)?;

    debouncer.watch(parent_a, RecursiveMode::NonRecursive)?;
    debouncer.watch(parent_b, RecursiveMode::NonRecursive)?;

    for result in rx {
        match result {
            Ok(events) => {
                let mut check_a: bool = false;
                let mut check_b: bool = false;

                for event in events {
                    if let EventKind::Access(_) = event.kind {
                        continue;
                    }

                    for path in &event.paths {
                        if path == &path_a { check_a = true; }
                        if path == &path_b { check_b = true; }
                    }
                }

                if check_a {
                    match compute_hash(&path_a) {
                        Ok(new_hash) => {
                            if new_hash != hash_a {
                                println!("File A changed (Hash: {:x}). Syncing to B...", new_hash);
                                hash_a = new_hash;

                                let content = std::fs::read_to_string(&path_a)?;
                                std::fs::write(&path_b, content)?;

                                hash_b = new_hash;
                            }
                        },
                        Err(e) => eprintln!("Error hashing A: {}", e),
                    }
                }

                if check_b {
                    match compute_hash(&path_b) {
                        Ok(new_hash) => {
                            if new_hash != hash_b {
                                println!("File B changed (Hash: {:x}). Syncing to A...", new_hash);
                                hash_b = new_hash;

                                let content = std::fs::read_to_string(&path_b)?;
                                std::fs::write(&path_a, content)?;

                                hash_a = new_hash;
                            }
                        },
                        Err(e) => eprintln!("Error hashing B: {}", e),
                    }
                }

            },
            Err(e) => println!("Watch error: {:?}", e),
        }
    }

    Ok(())
}