use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::config;

pub fn run(purge: bool) -> Result<()> {
    let dest = env::current_exe().context("找不到当前 ypdf 路径")?;
    let dest = dest.canonicalize().unwrap_or(dest);
    let config = if purge {
        Some(config::config_path()?)
    } else {
        None
    };
    apply(&dest, config.as_deref())
}

pub fn apply(binary: &Path, config: Option<&Path>) -> Result<()> {
    println!("binary={}", binary.display());
    if dest_not_writable(binary) {
        println!("status=need_write");
        bail!("当前二进制不可写: {}", binary.display());
    }

    remove_sidecars(binary);
    remove_current_binary(binary)?;

    if let Some(config) = config {
        println!("config={}", config.display());
        remove_config(config)?;
    }

    println!("status=uninstalled");
    Ok(())
}

fn dest_not_writable(dest: &Path) -> bool {
    let Some(dir) = dest.parent() else {
        return true;
    };
    if !dir.exists() {
        return true;
    }
    let probe = dir.join(".ypdf-write-probe");
    let ok = fs::write(&probe, b"ok").is_ok();
    let _ = fs::remove_file(&probe);
    !ok
}

fn sidecar_paths(binary: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    paths.push(binary.with_extension("exe.old"));
    if let Some(name) = binary.file_name().and_then(|name| name.to_str()) {
        if let Some(dir) = binary.parent() {
            paths.push(dir.join(format!(".{name}.new")));
        }
    }
    paths
}

fn remove_sidecars(binary: &Path) {
    for path in sidecar_paths(binary) {
        if path != binary {
            let _ = fs::remove_file(path);
        }
    }
}

fn remove_current_binary(dest: &Path) -> Result<()> {
    if !dest.exists() {
        return Ok(());
    }

    #[cfg(windows)]
    {
        let old = dest.with_extension("exe.old");
        let _ = fs::remove_file(&old);
        fs::rename(dest, &old).with_context(|| format!("无法改名 {}", dest.display()))?;
        if fs::remove_file(&old).is_err() {
            println!("leftover={}", old.display());
        }
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        fs::remove_file(dest).with_context(|| format!("无法删除 {}", dest.display()))?;
        Ok(())
    }
}

fn remove_config(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("无法删除 {}", path.display()))?;
    }
    if let Some(dir) = path.parent() {
        if dir.file_name().and_then(|name| name.to_str()) == Some("ypdf") {
            let _ = fs::remove_dir(dir);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!(
            "ypdf-uninstall-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn deletes_binary_keeps_config() {
        let dir = temp_dir("keep");
        let bin_dir = dir.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let bin = bin_dir.join("ypdf");
        fs::write(&bin, b"bin").unwrap();
        let cfg_dir = dir.join("ypdf");
        fs::create_dir_all(&cfg_dir).unwrap();
        let cfg = cfg_dir.join("config.toml");
        fs::write(&cfg, "apiKey = \"ypdf_x\"").unwrap();

        apply(&bin, None).unwrap();

        assert!(!bin.exists());
        assert!(cfg.exists());
    }

    #[test]
    fn purge_removes_config_and_empty_dir() {
        let dir = temp_dir("purge");
        let bin_dir = dir.join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let bin = bin_dir.join("ypdf");
        fs::write(&bin, b"bin").unwrap();
        fs::write(bin.with_extension("exe.old"), b"old").unwrap();
        let cfg_dir = dir.join("ypdf");
        fs::create_dir_all(&cfg_dir).unwrap();
        let cfg = cfg_dir.join("config.toml");
        fs::write(&cfg, "apiKey = \"ypdf_x\"").unwrap();

        apply(&bin, Some(&cfg)).unwrap();

        assert!(!bin.exists());
        assert!(!cfg.exists());
        assert!(!cfg_dir.exists());
        assert!(!bin.with_extension("exe.old").exists());
    }

    #[test]
    fn need_write_when_directory_is_readonly() {
        let dir = temp_dir("ro");
        let bin = dir.join("ypdf");
        fs::write(&bin, b"bin").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();
            let err = apply(&bin, None).unwrap_err();
            assert!(err.to_string().contains("不可写"));
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
            assert!(bin.exists());
        }
        #[cfg(not(unix))]
        {
            let _ = bin;
        }
    }
}
