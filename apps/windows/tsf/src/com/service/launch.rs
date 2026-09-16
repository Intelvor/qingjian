//! 连不上 Server 时自己拉起它。
//!
//! Server 只在登录时由「启动」文件夹的快捷方式拉起（Explorer 走 `ShellExecute`，见 `qingjian.iss`），
//! 中途挂了（崩溃 / 被杀 / 装完没重启）以前 DLL 只能静默吞键到下次登录。这里在连接失败后起一次与
//! DLL 同目录的 `qingjian-server.exe`：用 `ShellExecuteW` 而不是 `CreateProcess`——`uiAccess=true`
//! 的 exe 用 `CreateProcess` 拉不起来（报 740），ShellExecute 等同双击，两种构建都行。
//!
//! 两道闸门防重复启动：进程内的冷却时间（同一应用连敲只试一次），与跨进程的命名互斥体
//! （多个应用同时发现 Server 不在，只起一个）。找不到 exe / 起失败记日志返回 `false`，调用方退回原来的退避重连。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, w};

use crate::com::log::log;
use crate::com::module_path;

/// 同一进程内两次尝试拉起 Server 的最短间隔：连接失败每个键都会走到这里，别把应用砸出一串 Server。
const LAUNCH_COOLDOWN: Duration = Duration::from_secs(5);

/// 跨进程互斥体：多个应用的 DLL 同时发现 Server 不在时只起一个（第二个起来的抢不到管道会自己退出）。
const LAUNCH_MUTEX: windows::core::PCWSTR = w!("Local\\QingjianServerLaunch");

/// 上次尝试拉起的时间；本进程内所有文本服务实例共用。
static LAST_LAUNCH: Mutex<Option<Instant>> = Mutex::new(None);

/// 连不上 Server 时拉起它。已请求启动返回 `true`（下一键就该连上），没试 / 失败返回 `false`。
pub(super) fn launch_server() -> bool {
    if !cooldown_passed() {
        return false;
    }
    let Some(exe) = server_exe() else {
        log("找不到与 DLL 同目录的 qingjian-server.exe，不拉起");
        return false;
    };
    let Some(_mutex) = launch_mutex() else {
        // 别的进程正在起：不重复起，退回退避重连
        return false;
    };
    let file = HSTRING::from(exe.as_os_str());
    let workdir = exe
        .parent()
        .map(|dir| HSTRING::from(dir.as_os_str()))
        .unwrap_or_default();
    // 起完就返回：Server 一百多毫秒就监听管道，阻塞在应用 UI 线程上会被 TSF 看门狗切走输入法。
    let code = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(file.as_ptr()),
            PCWSTR::null(),
            PCWSTR(workdir.as_ptr()),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW 返回值 > 32 才算成功。
    if (code.0 as isize) > 32 {
        log("已请求启动 qingjian-server");
        true
    } else {
        log(&format!(
            "启动 qingjian-server 失败，返回值 {}",
            code.0 as isize
        ));
        false
    }
}

/// 进程内冷却：距上次尝试不到 [`LAUNCH_COOLDOWN`] 就不试。时间在尝试**之前**记，失败也冷却。
fn cooldown_passed() -> bool {
    let Ok(mut last) = LAST_LAUNCH.lock() else {
        return false;
    };
    if last.is_some_and(|at| at.elapsed() < LAUNCH_COOLDOWN) {
        return false;
    }
    *last = Some(Instant::now());
    true
}

/// 跨进程互斥体；已被别的进程持有（对方正在起 Server）返回 `None`。
fn launch_mutex() -> Option<LaunchMutex> {
    let handle = unsafe { CreateMutexW(None, false, LAUNCH_MUTEX) }.ok()?;
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        let _ = unsafe { CloseHandle(handle) };
        return None;
    }
    Some(LaunchMutex(handle))
}

/// 持有互斥体的句柄，析构时关闭。
struct LaunchMutex(HANDLE);

impl Drop for LaunchMutex {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn server_exe() -> Option<PathBuf> {
    let module = module_path().ok()?;
    Some(server_exe_path(Path::new(&module.to_string())))
}

/// 与 DLL 同目录的 `qingjian-server.exe`（安装器把两者装在同一目录）。
fn server_exe_path(module: &Path) -> PathBuf {
    module.with_file_name("qingjian-server.exe")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::server_exe_path;

    #[test]
    fn server_exe_sits_next_to_the_dll() {
        assert_eq!(
            server_exe_path(Path::new(
                r"D:\Program Files\Qingjian\qingjian_tsf-0.1.0-alpha.15-dev.dll"
            )),
            Path::new(r"D:\Program Files\Qingjian\qingjian-server.exe")
        );
    }
}
