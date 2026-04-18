use crate::io::xdg::na_backup_dir;
use anyhow::Result;
use std::env;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub fn discover_taskpaper_files(ext: &str) -> Result<Vec<PathBuf>> {
    discover_taskpaper_files_with_options(ext, 5, false)
}

pub fn discover_taskpaper_files_with_options(
    ext: &str,
    max_depth: usize,
    include_hidden: bool,
) -> Result<Vec<PathBuf>> {
    let cwd = env::current_dir()?;
    let mut files = Vec::new();

    for entry in WalkDir::new(&cwd).max_depth(max_depth) {
        let entry = entry?;
        let is_regular_file = entry.file_type().is_file();
        let is_file_symlink = entry.file_type().is_symlink()
            && std::fs::metadata(entry.path())
                .map(|meta| meta.is_file())
                .unwrap_or(false);
        if !is_regular_file && !is_file_symlink {
            continue;
        }
        if !include_hidden && is_hidden_entry(entry.path(), &cwd) {
            continue;
        }

        if entry
            .path()
            .extension()
            .is_some_and(|x| x == ext.trim_start_matches('.'))
        {
            files.push(entry.path().to_path_buf());
        }
    }

    files.sort();
    Ok(files)
}

fn is_hidden_entry(path: &Path, cwd: &Path) -> bool {
    path.strip_prefix(cwd)
        .ok()
        .into_iter()
        .flat_map(Path::components)
        .filter_map(|component| component.as_os_str().to_str())
        .any(|segment| segment.starts_with('.'))
}

pub fn backup_path(path: &Path) -> PathBuf {
    let backup_root = na_backup_dir();
    let relative = if path.is_absolute() {
        path.components().skip(1).collect::<PathBuf>()
    } else {
        path.to_path_buf()
    };

    let mut out = backup_root.join(relative);
    if let Some(file_name) = out.file_name().map(|f| f.to_string_lossy().to_string()) {
        out.set_file_name(format!("{file_name}.bak"));
    } else {
        out.push("backup.bak");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{backup_path, discover_taskpaper_files_with_options};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);
    static CWD_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn cwd_lock() -> &'static Mutex<()> {
        CWD_LOCK.get_or_init(|| Mutex::new(()))
    }

    fn fixture_dir() -> PathBuf {
        let mut path = std::env::temp_dir();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("valid monotonic time")
            .as_nanos();
        let seq = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!(
            "na_rust_discover_files_fixture_{}_{}_{}",
            std::process::id(),
            ts,
            seq
        ));
        path
    }

    #[test]
    fn backup_path_uses_xdg_data_home_when_available() {
        let sample = Path::new("/tmp/example.taskpaper");
        let xdg = std::env::temp_dir().join("na_rust_xdg_data_home_test");
        std::env::set_var("XDG_DATA_HOME", &xdg);
        let out = backup_path(sample);
        std::env::remove_var("XDG_DATA_HOME");

        assert!(out.starts_with(xdg.join("na").join("backup")));
        assert!(out.ends_with(Path::new("tmp/example.taskpaper.bak")));
    }

    #[test]
    fn discover_taskpaper_files_respects_depth_and_hidden_options() {
        let _guard = cwd_lock().lock().expect("cwd lock should be available");
        let root = fixture_dir();
        fs::create_dir_all(root.join("nested").join("deep")).expect("fixture dir should create");
        fs::create_dir_all(root.join(".hidden")).expect("hidden fixture dir should create");
        fs::write(root.join("root.taskpaper"), "Inbox:\n").expect("root fixture write");
        fs::write(root.join("nested").join("deep").join("deep.taskpaper"), "Inbox:\n")
            .expect("deep fixture write");
        fs::write(root.join(".hidden").join("hidden.taskpaper"), "Inbox:\n")
            .expect("hidden fixture write");

        let old_cwd = std::env::current_dir().expect("cwd should resolve");
        std::env::set_current_dir(&root).expect("fixture cwd should change");

        let shallow =
            discover_taskpaper_files_with_options("taskpaper", 2, false).expect("discover should work");
        let deep =
            discover_taskpaper_files_with_options("taskpaper", 5, false).expect("discover should work");
        let with_hidden =
            discover_taskpaper_files_with_options("taskpaper", 5, true).expect("discover should work");

        std::env::set_current_dir(old_cwd).expect("cwd should restore");
        fs::remove_dir_all(root).ok();

        assert_eq!(shallow.len(), 1);
        assert!(shallow.iter().any(|p| p.ends_with("root.taskpaper")));
        assert_eq!(deep.len(), 2);
        assert!(deep.iter().any(|p| p.ends_with("deep.taskpaper")));
        assert_eq!(with_hidden.len(), 3);
        assert!(with_hidden.iter().any(|p| p.ends_with("hidden.taskpaper")));
    }

    #[cfg(unix)]
    #[test]
    fn discover_taskpaper_files_includes_symlinked_files() {
        let _guard = cwd_lock().lock().expect("cwd lock should be available");
        let root = fixture_dir();
        fs::create_dir_all(&root).expect("fixture dir should create");
        fs::write(root.join("real.taskpaper"), "Inbox:\n").expect("real fixture write");
        std::os::unix::fs::symlink(root.join("real.taskpaper"), root.join("linked.taskpaper"))
            .expect("symlink fixture create");

        let old_cwd = std::env::current_dir().expect("cwd should resolve");
        std::env::set_current_dir(&root).expect("fixture cwd should change");
        let files =
            discover_taskpaper_files_with_options("taskpaper", 3, false).expect("discover should work");
        std::env::set_current_dir(old_cwd).expect("cwd should restore");
        fs::remove_dir_all(root).ok();

        assert!(files.iter().any(|p| p.ends_with("real.taskpaper")));
        assert!(files.iter().any(|p| p.ends_with("linked.taskpaper")));
    }
}
