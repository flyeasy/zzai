//! 跨平台的小文件工具（DESIGN M3：core 要能为 Windows 复用）
//!
//! 权限相关的做法在 Unix / Windows 上是两套语义：
//! - Unix：`chmod 0600`，只有文件主能读写。
//! - Windows：靠文件 ACL（默认继承用户目录权限即可），没有 chmod 的等价物；
//!   真要收紧得调 `SetNamedSecurityInfo`，超出个人自用工具的合理投入，先不做。
//!
//! 所以这里只承诺"能写、能原子替换"；收紧权限只在 Unix 上尽力而为，绝不因为做不到而让写入失败。

use std::io::Write;
use std::path::Path;

/// 尽量把文件权限收成只有自己能读写（Windows 上为 no-op）
pub fn restrict(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// 原子写：先写 `<path>.tmp`，再 rename 覆盖；Unix 上全程 0600
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        restrict(&tmp);
        f.write_all(bytes)?;
        f.flush()?;
    }
    std::fs::rename(&tmp, path)
}

/// 原子写文本
pub fn write_text_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    write_atomic(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_replaces_atomically() {
        let dir = std::env::temp_dir().join(format!("eryu-fsutil-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("a.txt");
        write_text_atomic(&p, "第一版").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "第一版");
        write_text_atomic(&p, "第二版").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "第二版");
        assert!(!p.with_extension("tmp").exists(), "临时文件应被 rename 掉");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "Unix 上应为 0600");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn creates_parent_dirs() {
        let dir = std::env::temp_dir().join(format!("eryu-fsutil-nest-{}", std::process::id()));
        let p = dir.join("deep").join("b.txt");
        write_text_atomic(&p, "x").unwrap();
        assert!(p.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
