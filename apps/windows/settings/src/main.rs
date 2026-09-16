//! 青简 Windows 设置界面入口：左侧导航栏 + 各分节表单，读写 `%APPDATA%\Qingjian\config.toml`。
//! UI 用 Windows Reactor；非 Windows 编成空壳，让工作区能整体编译。
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod panel;

#[cfg(windows)]
fn main() {
    // 已经开着一个就不再开第二个（状态条齿轮那条路会先把已有窗口叫到前台，见 Server 的 `open_settings`）。
    if already_running() {
        return;
    }
    if let Err(error) = windows_reactor::App::run_component::<panel::Settings>(()) {
        eprintln!("设置界面启动失败: {error:?}");
    }
}

/// 本登录会话（`Local\`）里已经有一个设置程序在跑？互斥体拿到手就活到进程结束，不显式关。
#[cfg(windows)]
fn already_running() -> bool {
    use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::core::{PCWSTR, w};

    /// 只有设置程序用的名字。
    const INSTANCE: PCWSTR = w!("Local\\QingjianSettings");

    let Ok(handle) = (unsafe { CreateMutexW(None, false, INSTANCE) }) else {
        // 建不出来（权限之类）就别拦着用户用设置。
        return false;
    };
    if unsafe { windows::Win32::Foundation::GetLastError() } == ERROR_ALREADY_EXISTS {
        let _ = unsafe { CloseHandle(handle) };
        return true;
    }
    // 拿到手的句柄故意不关：「关了互斥体就没了」，下次启动就挡不住了。`HANDLE` 是 Copy，
    // 出了作用域也不会自动关，进程活着的这段它就是那面旗子。
    false
}

#[cfg(not(windows))]
fn main() {
    eprintln!("qingjian-settings 仅支持 Windows");
}
