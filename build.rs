//! 构建脚本：Windows 目标嵌入应用图标（.ico）与版本信息。
//!
//! 注意：不能用 `#[cfg(windows)]` 判断目标平台——交叉编译时 build.rs
//! 由宿主编译器运行，`cfg(windows)` 反映的是宿主（如 Linux）而非目标。
//! 必须用 cargo 注入的环境变量 `CARGO_CFG_TARGET_OS` 判断目标 OS。

fn main() {
    // 图标文件变化时自动重新嵌入（不声明则 cargo 不会因 ico 变化重跑本脚本）
    println!("cargo:rerun-if-changed=assets/serial-term.ico");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        // 非 Windows 目标无需嵌入 .ico
        return;
    }
    let mut res = winres::WindowsResource::new();
    // 交叉编译时显式指定 mingw 的 windres/ar，避免依赖 PATH 中的默认名
    if let Ok(home) = std::env::var("HOME") {
        let wr = format!("{}/tools/mingw/usr/bin/x86_64-w64-mingw32-windres", home);
        let ar = format!("{}/tools/mingw/usr/bin/x86_64-w64-mingw32-ar", home);
        if std::path::Path::new(&wr).exists() {
            res.set_windres_path(&wr);
        }
        if std::path::Path::new(&ar).exists() {
            res.set_ar_path(&ar);
        }
    }
    res.set_icon("assets/serial-term.ico");
    // 版本信息
    res.set("FileDescription", "Serial Terminal");
    res.set("ProductName", "Serial Terminal");
    res.set("FileVersion", env!("CARGO_PKG_VERSION"));
    res.set("ProductVersion", env!("CARGO_PKG_VERSION"));
    res.set("OriginalFilename", "serial-term.exe");
    match res.compile() {
        Ok(()) => {
            // winres 在 mingw/gnu 工具链下生成 `resource.o`，并打包为静态库；
            // 但其中对象无被引用符号，链接器不会提取它，导致 .rsrc 段被丢弃，
            // 必须用绝对路径直接传该对象并强制包含，图标/版本信息才嵌入 exe。
            // 而在 MSVC 工具链下 winres 生成 `resource.lib` 并自动通过
            // `cargo:rustc-link-lib` 链接，资源段由链接器特殊处理（不会被丢弃），
            // 此时不存在 `resource.o`。若仍强制链接该文件会导致 LNK1181。
            // 因此仅当 `resource.o` 确实存在（mingw 工具链）时才强制链接。
            let out = std::env::var("OUT_DIR").unwrap_or_default();
            let res_o = std::path::Path::new(&out).join("resource.o");
            if res_o.exists() {
                println!("cargo:rustc-link-arg={}", res_o.display());
            }
        }
        Err(e) => panic!("winres 编译资源失败: {}", e),
    }
}
