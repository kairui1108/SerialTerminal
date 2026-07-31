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
            // winres 打包为 libresource.a 用 `-l static=resource` 链接，但其中对象
            // 无被引用符号，链接器不会提取它，导致 .rsrc 被丢弃。
            // 直接用绝对路径传资源对象并强制包含，确保图标/版本信息嵌入 exe。
            let out = std::env::var("OUT_DIR").unwrap_or_default();
            let res_o = format!("{}/resource.o", out);
            println!("cargo:rustc-link-arg={}", res_o);
        }
        Err(e) => panic!("winres 编译资源失败: {}", e),
    }
}
