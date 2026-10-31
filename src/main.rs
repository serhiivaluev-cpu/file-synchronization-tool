use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn write_log(message: &str) -> io::Result<()> {
    use std::fs::OpenOptions;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("sync.log")?;

    writeln!(file, "{}", message)?;

    Ok(())
}

fn scan_folder(folder: &Path, root: &Path) -> io::Result<HashSet<PathBuf>> {
    let mut files = HashSet::new();

    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            files.extend(scan_folder(&path, root)?);
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).map_err(io::Error::other)?;
            files.insert(relative.to_path_buf());
        }
    }

    Ok(files)
}

fn files_equal(file1: &Path, file2: &Path) -> io::Result<bool> {
    if fs::metadata(file1)?.len() != fs::metadata(file2)?.len() {
        return Ok(false);
    }

    Ok(fs::read(file1)? == fs::read(file2)?)
}

fn copy_missing_file(
    source_root: &Path,
    destination_root: &Path,
    relative: &Path,
) -> io::Result<()> {
    let source = source_root.join(relative);
    let destination = destination_root.join(relative);

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }

    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Destination already exists",
        ));
    }

    fs::copy(&source, &destination)?;
    println!("Copied: {}", relative.display());
    write_log(&format!("COPIED: {}", relative.display()))?;

    Ok(())
}

fn resolve_conflict(
    source_root: &Path,
    destination_root: &Path,
    backup_root: &Path,
    relative: &Path,
) -> io::Result<()> {
    let source = source_root.join(relative);
    let destination = destination_root.join(relative);

    // Резервна копія зберігає структуру вкладених папок.

    let original_backup = backup_root.join(relative);

    if let Some(parent) = original_backup.parent() {
        fs::create_dir_all(parent)?;
    }

    // Шукаємо вільне ім'я для нової резервної копії.
    let mut backup = original_backup.clone();
    let mut number = 1;

    while backup.exists() {
        let name = original_backup.file_name().unwrap().to_string_lossy();

        backup = original_backup.with_file_name(format!("{}.bak{}", name, number));

        number += 1;
    }

    // Спочатку зберігаємо стару версію.
    fs::copy(&destination, &backup)?;

    // Потім оновлюємо файл.
    fs::copy(&source, &destination)?;

    println!("Updated: {}", relative.display());
    write_log(&format!("UPDATED: {}", relative.display()))?;
    println!("Backup saved: {}", backup.display());

    Ok(())
}

fn main() -> io::Result<()> {
    println!("=== File Synchronization Tool ===");

    let args: Vec<String> = env::args().collect();

    if args.len() != 3 && args.len() != 4 {
        println!("Usage: file_synchronization_tool <folder1> <folder2> [--resolve-from-first]");
        return Ok(());
    }

    let resolve = args.len() == 4;

    if resolve && args[3] != "--resolve-from-first" {
        println!("Error: Unknown option!");
        return Ok(());
    }

    let folder1 = Path::new(&args[1]);
    let folder2 = Path::new(&args[2]);

    if !folder1.is_dir() || !folder2.is_dir() {
        println!("Error: Both folders must exist!");
        return Ok(());
    }

    // Забороняємо синхронізацію папки із самою собою
    // або з її вкладеною папкою.
    let path1 = fs::canonicalize(folder1)?;
    let path2 = fs::canonicalize(folder2)?;

    if path1.starts_with(&path2) || path2.starts_with(&path1) {
        println!("Error: Folders must be separate!");
        return Ok(());
    }

    let files1 = scan_folder(folder1, folder1)?;
    let files2 = scan_folder(folder2, folder2)?;

    let mut only_first: Vec<_> = files1.difference(&files2).collect();
    let mut only_second: Vec<_> = files2.difference(&files1).collect();
    let mut common: Vec<_> = files1.intersection(&files2).collect();

    only_first.sort();
    only_second.sort();
    common.sort();

    println!("\nSynchronizing files...\n");

    for file in only_first {
        copy_missing_file(folder1, folder2, file)?;
    }

    for file in only_second {
        copy_missing_file(folder2, folder1, file)?;
    }

    let mut conflicts = 0;

    for file in common {
        let path1 = folder1.join(file);
        let path2 = folder2.join(file);

        if !files_equal(&path1, &path2)? {
            if resolve {
                resolve_conflict(folder1, folder2, Path::new("backups/automatic"), file)?;
            } else {
                println!("Conflict detected: {}", file.display());
                write_log(&format!("CONFLICT: {}", file.display()))?;
                conflicts += 1;
            }
        }
    }

    println!("\nSynchronization completed!");
    println!("Unresolved conflicts: {}", conflicts);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_directory() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "file_sync_test_{}_{}",
            std::process::id(),
            timestamp
        ))
    }

    #[test]
    fn test_identical_files() {
        let dir = test_directory();
        fs::create_dir_all(&dir).unwrap();

        let file1 = dir.join("file1.txt");
        let file2 = dir.join("file2.txt");

        fs::write(&file1, "Hello").unwrap();
        fs::write(&file2, "Hello").unwrap();

        assert!(files_equal(&file1, &file2).unwrap());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_different_files() {
        let dir = test_directory();
        fs::create_dir_all(&dir).unwrap();

        let file1 = dir.join("file1.txt");
        let file2 = dir.join("file2.txt");

        fs::write(&file1, "Hello").unwrap();
        fs::write(&file2, "World!").unwrap();

        assert!(!files_equal(&file1, &file2).unwrap());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_missing_file() {
        let dir = test_directory();
        fs::create_dir_all(&dir).unwrap();

        let file1 = dir.join("file1.txt");
        let file2 = dir.join("missing.txt");

        fs::write(&file1, "Hello").unwrap();

        assert!(files_equal(&file1, &file2).is_err());

        fs::remove_dir_all(dir).unwrap();
    }
}
