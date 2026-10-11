//! Atomically persist locally sensitive configuration without a world-readable
//! creation window or a partially truncated destination file.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

pub fn write_private(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("无法确定配置目录")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let name = path.file_name().ok_or("无效的配置文件路径")?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    let result = (|| -> std::io::Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        // Best effort: persist the rename as well as the file contents.
        if let Ok(dir) = fs::File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_private_file_is_atomic_and_preserves_permissions() {
        let dir = std::env::temp_dir().join(format!("socksnode-config-{}", uuid::Uuid::new_v4()));
        let path = dir.join("nodes.json");
        write_private(&path, b"first").unwrap();
        write_private(&path, b"updated").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"updated");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
}
