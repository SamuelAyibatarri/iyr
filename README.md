# iyr 🦀

**iyr** is a smart, bidirectional file sync tool written in Rust. It keeps two files in perfect sync, acting like a "virtual hard link" that works across directories and partitions.

## 🚀 Why?
Syncing code manually is painful. Generic watchers trigger infinite loops. `iyr` solves this by using **hashing** instead of timestamps to ensure changes are propagated only when the content actually changes.

## ✨ Features
- **Bidirectional Sync:** A <-> B.
- **Loop Prevention:** Uses CRC32 checksums to stop infinite sync cycles.
- **Smart Watching:** Ignores file "reads" to prevent false triggers.
- **Conflict Resolution:** Safely handles non-identical files with `--overwrite`.

## 📦 Installation
Clone the repo and install the binary globally:
```bash
cargo install --path . 
```

## 🛠 Usage

### Basic Sync

Run `iyr` with two file paths to start a bidirectional watcher. If you edit one, the other updates instantly.

```bash
iyr ./backend/src/Interfaces.ts ./frontend/src/Interfaces.ts
```

### Conflict Resolution (overwrite)

If the two files have different content, `iyr` will refuse to run to prevent accidental data loss. Use the `--overwrite` flag to resolve this.

**Note:** This will create backups of both files (e.g., `file_backup.txt`) before clearing the originals to start fresh.

```bash
iyr ./file_a.txt ./file_b.txt --overwrite
```