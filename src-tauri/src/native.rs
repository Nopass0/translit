//! Windows helper process calls, always without a visible console window.
use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command},
};
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Executes a native helper command and returns UTF-8 stdout or a detailed error.
pub fn call(executable: &Path, arguments: &[String]) -> Result<String, String> {
    let result = Command::new(executable)
        .args(arguments)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}
/// Starts a watchdog independently so it can resume the game after app failure.
pub fn watchdog(executable: &Path, pid: u32, render: bool) -> Result<Child, String> {
    Command::new(executable)
        .args([
            if render { "pause-render" } else { "pause" },
            &pid.to_string(),
            &std::process::id().to_string(),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| e.to_string())
}
/// Returns the same per-process control directory used by the injected DLL.
pub fn directory(pid: u32) -> PathBuf {
    std::env::temp_dir().join(format!("translit-native-v2-{pid}"))
}
