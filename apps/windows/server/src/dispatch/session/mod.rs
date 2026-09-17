//! 会话：DLL 每个应用线程一条，Server 记宿主应用；焦点在哪个会话，组句就属于谁。

mod info;

use qingjian_platform::protocol::SessionId;

pub(super) use self::info::SessionInfo;
use super::Router;

/// 前台窗口的 exe 文件名（`Code.exe`）；查不到时为 `None`。
///
/// 中英模式只采纳「该会话的宿主就是前台应用」的上报：每个加载了输入法的应用都在轮询上报，
/// 后台上报会覆盖状态条（表现是中英来回横跳、在状态条上切了又被改回去）。前台让 Server 自己判断，
/// 不信 DLL 的自我判断——Chromium 系（Edge / Chrome）的 TSF 跑在浏览器进程，前台窗口却可能属于
/// 它的 renderer 子进程，按 pid 比会对不上。
pub(super) fn foreground_app_name() -> Option<String> {
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    use windows::core::PWSTR;

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buffer = [0u16; 260];
    let mut len = buffer.len() as u32;
    let queried = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
    };
    let _ = unsafe { windows::Win32::Foundation::CloseHandle(process) };
    queried.ok()?;
    String::from_utf16_lossy(&buffer[..len as usize])
        .rsplit(['\\', '/'])
        .next()
        .map(str::to_owned)
}

impl Router {
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// 聚焦会话所在应用的 exe 名；DLL 没报时为 `None`。
    pub(super) fn focused_app(&self) -> Option<&str> {
        self.focused
            .and_then(|session| self.sessions.get(&session))
            .and_then(|info| info.app.as_deref())
    }

    pub(super) fn ensure_focus(&mut self, session: SessionId) {
        if self.focused != Some(session) {
            self.reset_composition();
            self.focused = Some(session);
            let app = self.focused_app().map(str::to_owned);
            self.engine.set_application(app);
            let private = self.focused_private();
            self.engine.set_private(private);
        }
    }

    /// 当前聚焦的会话在私密输入框里（Engine 不学不记不发云端）。
    pub fn is_private(&self) -> bool {
        self.engine.is_private()
    }

    fn focused_private(&self) -> bool {
        self.focused
            .and_then(|session| self.sessions.get(&session))
            .is_some_and(|info| info.private)
    }

    /// DLL 报来该会话的输入框私密与否变了：记下；是当前会话就立刻让 Engine 进 / 出私密。
    pub(super) fn set_privacy(&mut self, session: SessionId, private: bool) {
        if let Some(info) = self.sessions.get_mut(&session) {
            info.private = private;
        }
        if self.focused == Some(session) {
            self.engine.set_private(private);
        }
    }

    /// 焦点离开：把缓冲区原样交出并清组句。组句不属于 `session` 时只清不交，别把 A 应用的拼音落进 B。
    pub(super) fn commit_raw_for(&mut self, session: SessionId) -> Option<String> {
        let text = (self.focused == Some(session) && !self.engine.composition().is_empty())
            .then(|| self.engine.take_raw());
        self.reset_composition();
        text
    }

    /// 清掉组句、展示状态、在飞的云联想与翻译评审，收起候选窗口。
    pub(super) fn reset_composition(&mut self) {
        self.engine.break_chain();
        self.engine.clear();
        self.cancel_prediction();
        self.stop_rescoring();
        self.composed = None;
        self.translation = None;
        self.pending_selection = None;
        self.sentence = None;
        // 还没被 DLL 取走的点选文本 / 等待提示：换会话、换输入法了就别带给下一个应用。
        self.pending_commit = None;
        self.sentence_pending = None;
        self.notice = None;
        self.highlight = 0;
        self.navigated = false;
        self.hide_candidate_window();
    }
}
