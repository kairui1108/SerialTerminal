/// 控制台初始化：Windows 下设置 UTF-8 代码页与 VT 处理；其他平台无需处理。
#[cfg(windows)]
pub fn setup_console() -> anyhow::Result<()> {
    use windows_sys::Win32::System::Console::{SetConsoleCP, SetConsoleOutputCP};
    unsafe {
        let _ = SetConsoleOutputCP(65001); // CP_UTF8
        let _ = SetConsoleCP(65001);
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn setup_console() -> anyhow::Result<()> {
    Ok(())
}
