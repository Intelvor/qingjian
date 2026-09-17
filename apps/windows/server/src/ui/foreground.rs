//! 前台窗口跟踪：状态条要跟前台应用的中英模式走，而「谁在前台」只有系统知道。
//!
//! `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` 让系统在前台变化时**主动回调**装了钩子的线程
//! （UI 线程有消息泵），比自己去 `GetForegroundWindow` 轮询准，也不需要在每个应用进程里各判一次：
//! 回调只负责收集前台窗口的归属线索（线程 id + 进程 id），认人交给 Router 对着会话表做
//! （[`crate::dispatch::Router::handle_foreground`]）。
//!
//! 线索收三层，按可信度从高到低：前台窗口本身、它的根祖先、它的后代窗口。后两层是兜底 ——
//! 商店应用的前台窗口是 `ApplicationFrameHost.exe` 的框架窗，真正装了输入法的那个窗口是它的后代；
//! 反过来，弹出窗 / 子窗口拿到前台时，装了输入法的是它的根窗口。

use std::cell::RefCell;

use windows::Win32::Foundation::{FALSE, GetLastError, HWND, LPARAM, TRUE};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    EVENT_SYSTEM_FOREGROUND, EnumChildWindows, GA_ROOT, GetAncestor, GetForegroundWindow,
    GetWindowThreadProcessId, WINEVENT_OUTOFCONTEXT,
};
use windows::core::{Error, HRESULT, Result};

use super::ForegroundEvents;

/// 归属线索最多收多少条：够覆盖框架窗加一堆子窗口，也免得在窗口特别多的宿主上久留。
const MAX_HINTS: usize = 32;

thread_local! {
    /// 本线程的回调。钩子回调是 `extern "system"` 的裸函数，只能经线程局部状态拿到它。
    static SINK: RefCell<Option<ForegroundEvents>> = const { RefCell::new(None) };
}

/// WinEvent 钩子的守卫：`Drop` 里摘钩子。UI 线程随进程存活，正常不析构。
pub(super) struct ForegroundHook {
    hook: HWINEVENTHOOK,
}

impl ForegroundHook {
    /// 在**调用线程**上装钩子（该线程必须有消息泵，否则回调永远不来）。
    /// 装好立刻报一次当前前台：钩子只在「变化」时回调，而 Server 起来时前台早就在那儿了。
    pub(super) fn install(events: ForegroundEvents) -> Result<Self> {
        SINK.with(|sink| *sink.borrow_mut() = Some(events));
        let hook = unsafe {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                None,
                Some(on_foreground),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        if hook.is_invalid() {
            SINK.with(|sink| *sink.borrow_mut() = None);
            let last = unsafe { GetLastError() };
            return Err(Error::from_hresult(HRESULT::from_win32(last.0)));
        }
        report(unsafe { GetForegroundWindow() });
        Ok(Self { hook })
    }
}

impl Drop for ForegroundHook {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWinEvent(self.hook);
        }
        SINK.with(|sink| *sink.borrow_mut() = None);
    }
}

/// 系统报来前台变了。
unsafe extern "system" fn on_foreground(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    report(hwnd);
}

/// 收集 `hwnd` 的归属线索交给 Router。拿不到前台窗口（切换途中的一瞬）就什么都不做，保持原样。
fn report(hwnd: HWND) {
    if hwnd.is_invalid() {
        return;
    }
    let mut hints = Vec::new();
    push_identity(&mut hints, hwnd);
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    if !root.is_invalid() {
        push_identity(&mut hints, root);
    }
    unsafe {
        let _ = EnumChildWindows(
            Some(hwnd),
            Some(collect_identity),
            LPARAM(&raw mut hints as isize),
        );
    }
    SINK.with(|sink| {
        if let Some(events) = sink.borrow().as_ref() {
            events(hints);
        }
    });
}

/// 收一个窗口的 `(线程 id, 进程 id)`，去重（框架窗与它的子窗常在同一条线程上）。
fn push_identity(hints: &mut Vec<(u32, u32)>, hwnd: HWND) {
    let mut pid = 0u32;
    let tid = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let identity = (tid, pid);
    if !hints.contains(&identity) {
        hints.push(identity);
    }
}

/// 枚举后代窗口的回调：收归属线索，收满就停下枚举。
unsafe extern "system" fn collect_identity(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    // 安全：`lparam` 是 [`report`] 里那个局部 `Vec` 的地址，枚举是同步的，回调活不过它。
    let hints = unsafe { &mut *(lparam.0 as *mut Vec<(u32, u32)>) };
    if hints.len() >= MAX_HINTS {
        return FALSE;
    }
    push_identity(hints, hwnd);
    TRUE
}
