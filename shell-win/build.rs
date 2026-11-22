//! 把 app.rc（图标 + 版本信息）编成 .res 再交给链接器，让 exe 在资源管理器/开始菜单里
//! 显示我们自己的图标。交叉编译走 llvm-rc；找不到或编不出来就跳过——不阻断构建。

use std::path::PathBuf;
use std::process::Command;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
        return;
    }
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let rc = dir.join("app.rc");
    let res = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app.res");
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets/eryu.ico");

    let mut tools: Vec<String> = Vec::new();
    if let Ok(t) = std::env::var("ERYU_LLVM_RC") {
        tools.push(t);
    }
    tools.push("/opt/homebrew/opt/llvm/bin/llvm-rc".into());
    tools.push("llvm-rc".into());

    let mut last = String::from("没有可用的 llvm-rc");
    for tool in &tools {
        let _ = std::fs::remove_file(&res);
        let out = Command::new(tool)
            .arg(format!("/FO{}", res.display()))
            .arg("app.rc")
            .current_dir(&dir)
            .output();
        match out {
            Ok(o) if o.status.success() && res.exists() => {
                println!("cargo:rustc-link-arg={}", res.display());
                return;
            }
            Ok(o) => {
                last = format!(
                    "{tool} 退出码 {}：{}",
                    o.status,
                    String::from_utf8_lossy(&o.stderr).trim()
                );
            }
            Err(e) => last = format!("{tool} 起不来：{e}"),
        }
    }
    println!("cargo:warning=exe 不带图标资源（{last}）");
}
