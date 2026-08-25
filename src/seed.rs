use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use crate::common::{command_with_core_dump_off, has_ext, sha256_file, AppPaths};
use crate::target::{default_seed_dir, seed_ext, target_label, TargetKind};

pub(crate) fn run_seed_sync(
    app_paths: &AppPaths,
    target: &TargetKind,
    from: &Path,
    to: Option<&Path>,
    harness_filter: bool,
) -> Result<(), String> {
    if !from.exists() || !from.is_dir() {
        return Err(format!("source dir is invalid: {}", from.display()));
    }
    let ext = seed_ext(target);
    let dest_dir = match to {
        Some(path) => path.to_path_buf(),
        None => default_seed_dir(app_paths, target),
    };
    fs::create_dir_all(&dest_dir)
        .map_err(|e| format!("failed to create dest dir '{}': {e}", dest_dir.display()))?;

    let (mut existing_hashes, dest_hash_errors) =
        collect_existing_hashes_with(&dest_dir, ext, sha256_file)?;

    let mut scanned = 0usize;
    let mut matched_ext = 0usize;
    let mut copied = 0usize;
    let mut dup_skipped = 0usize;
    let mut invalid_skipped = 0usize;
    let mut error_skipped = 0usize;

    for entry in fs::read_dir(from)
        .map_err(|e| format!("failed to read source dir '{}': {e}", from.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read source entry: {e}"))?;
        let src = entry.path();
        if !src.is_file() {
            continue;
        }
        scanned += 1;
        if !has_ext(&src, ext) {
            continue;
        }
        matched_ext += 1;
        let hash = match sha256_file(&src) {
            Ok(h) => h,
            Err(_) => {
                error_skipped += 1;
                continue;
            }
        };
        if existing_hashes.contains(&hash) {
            dup_skipped += 1;
            continue;
        }
        if harness_filter {
            let valid = seed_harness_validate(target, &src)?;
            if !valid {
                invalid_skipped += 1;
                continue;
            }
        }
        let dest = unique_dest_path(&dest_dir, &src, ext)?;
        fs::copy(&src, &dest).map_err(|e| {
            format!(
                "failed to copy '{}' -> '{}': {e}",
                src.display(),
                dest.display()
            )
        })?;
        existing_hashes.insert(hash);
        copied += 1;
    }

    println!("[seed sync] done");
    println!("target: {}", target_label(target));
    println!("from: {}", from.display());
    println!("to: {}", dest_dir.display());
    println!("scanned: {scanned}");
    println!("matched_ext: {matched_ext}");
    println!("copied: {copied}");
    println!("dup_skipped: {dup_skipped}");
    println!("invalid_skipped: {invalid_skipped}");
    println!("error_skipped: {error_skipped}");
    println!("dest_hash_errors: {dest_hash_errors}");
    Ok(())
}

fn collect_existing_hashes_with(
    dir: &Path,
    ext: &str,
    hash: impl Fn(&Path) -> Result<String, String>,
) -> Result<(HashSet<String>, usize), String> {
    let mut hashes = HashSet::new();
    let mut errors = 0usize;
    for entry in
        fs::read_dir(dir).map_err(|e| format!("failed to read dest dir '{}': {e}", dir.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read dest entry: {e}"))?;
        let path = entry.path();
        if !path.is_file() || !has_ext(&path, ext) {
            continue;
        }
        // A24: a destination file that cannot be hashed used to drop out of the
        // known-hash set silently, so an identical source file was copied in again
        // as if it were new.
        match hash(&path) {
            Ok(h) => {
                hashes.insert(h);
            }
            Err(e) => {
                eprintln!(
                    "[seed sync] warning: cannot hash existing seed '{}': {e}",
                    path.display()
                );
                errors += 1;
            }
        }
    }
    Ok((hashes, errors))
}

/// A22: a file the tool could not hash still counts toward `total` but never
/// reaches the unique set, so `total - unique` reported it as a duplicate of
/// something - while hash_errors on the next line said the opposite.
fn duplicate_count(total: usize, hash_errors: usize, unique: usize) -> usize {
    total.saturating_sub(hash_errors).saturating_sub(unique)
}

pub(crate) fn run_seed_stats(
    app_paths: &AppPaths,
    target: &TargetKind,
    dir: Option<&Path>,
) -> Result<(), String> {
    let ext = seed_ext(target);
    let dir = match dir {
        Some(path) => path.to_path_buf(),
        None => default_seed_dir(app_paths, target),
    };
    if !dir.exists() || !dir.is_dir() {
        return Err(format!("seed dir is invalid: {}", dir.display()));
    }

    let mut total = 0usize;
    let mut unique = HashSet::new();
    let mut hash_errors = 0usize;
    let mut valid = 0usize;
    let mut invalid = 0usize;
    let mut validated = 0usize;
    let mut validation_errors = 0usize;
    for entry in fs::read_dir(&dir)
        .map_err(|e| format!("failed to read seed dir '{}': {e}", dir.display()))?
    {
        let entry = entry.map_err(|e| format!("failed to read seed entry: {e}"))?;
        let path = entry.path();
        if !path.is_file() || !has_ext(&path, ext) {
            continue;
        }
        total += 1;
        match sha256_file(&path) {
            Ok(h) => {
                unique.insert(h);
            }
            Err(_) => hash_errors += 1,
        }
        match seed_harness_validate(target, &path) {
            Ok(true) => {
                valid += 1;
                validated += 1;
            }
            Ok(false) => {
                invalid += 1;
                validated += 1;
            }
            // harness 실행 자체가 깨지면 valid/invalid 판정 불가. valid_ratio 왜곡
            // 방지를 위해 validated에 포함시키지 않고 별도 카운트로 가시화.
            Err(_) => validation_errors += 1,
        }
    }
    let deduped = unique.len();
    let duplicates = duplicate_count(total, hash_errors, deduped);
    let valid_ratio = if validated == 0 {
        0.0
    } else {
        valid as f64 / validated as f64
    };

    println!("[seed stats] done");
    println!("target: {}", target_label(target));
    println!("dir: {}", dir.display());
    println!("total: {total}");
    println!("unique: {deduped}");
    println!("duplicates: {duplicates}");
    println!("valid: {valid}");
    println!("invalid: {invalid}");
    println!("validated: {validated}");
    println!("valid_ratio: {:.4}", valid_ratio);
    println!("hash_errors: {hash_errors}");
    println!("validation_errors: {validation_errors}");
    Ok(())
}

fn unique_dest_path(dest_dir: &Path, src: &Path, ext: &str) -> Result<PathBuf, String> {
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("seed")
        .to_string();
    let mut candidate = dest_dir.join(format!("{stem}.{ext}"));
    let mut n = 1usize;
    while candidate.exists() {
        candidate = dest_dir.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    Ok(candidate)
}

fn seed_harness_validate(target: &TargetKind, input: &Path) -> Result<bool, String> {
    let exe = std::env::current_exe().map_err(|e| format!("failed to resolve current exe: {e}"))?;
    let out = command_with_core_dump_off(&exe)
        .arg("harness")
        .arg("--target")
        .arg(target_label(target))
        .arg("--input")
        .arg(input)
        .output()
        .map_err(|e| {
            format!(
                "failed to execute harness validation for '{}': {e}",
                input.display()
            )
        })?;
    Ok(out.status.success())
}

#[cfg(test)]
mod tests {
    use super::{collect_existing_hashes_with, duplicate_count};
    use std::fs;
    use std::path::{Path, PathBuf};

    // A22: a file the tool could not hash contributes to `total` but never to the
    // unique set, so `total - unique` reported it as a duplicate of something. The
    // corpus then looks more redundant than it is, and hash_errors right below it
    // says the opposite.
    #[test]
    fn duplicates_do_not_absorb_the_files_that_could_not_be_hashed() {
        assert_eq!(duplicate_count(5, 2, 3), 0, "3 hashed, 2 unreadable, 0 repeats");
        assert_eq!(duplicate_count(5, 0, 3), 2, "3 unique out of 5 readable files");
        assert_eq!(duplicate_count(3, 3, 0), 0, "nothing readable, nothing duplicated");
        assert_eq!(duplicate_count(2, 5, 0), 0, "never underflows");
    }

    // A24: a destination file that cannot be hashed dropped out of the known-hash
    // set with no trace, so an identical source file was copied in again as if it
    // were new.
    #[test]
    fn a_destination_file_that_cannot_be_hashed_is_counted() {
        let dir = temp_dir("seed-dest-hash-errors");
        fs::create_dir_all(&dir).expect("create dir");
        fs::write(dir.join("a.onnx"), b"a").expect("write a");
        fs::write(dir.join("b.onnx"), b"b").expect("write b");
        fs::write(dir.join("notes.txt"), b"skip").expect("write notes");

        let hash = |path: &Path| -> Result<String, String> {
            if path.ends_with("b.onnx") {
                Err("permission denied".to_string())
            } else {
                Ok("hash-of-a".to_string())
            }
        };
        let (hashes, errors) = collect_existing_hashes_with(&dir, "onnx", hash).expect("scan");

        assert_eq!(errors, 1, "the unreadable destination file must be counted");
        assert_eq!(hashes.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("tool-{name}-{}-{nanos}", std::process::id()))
    }
}
