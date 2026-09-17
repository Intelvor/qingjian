//! 会话：DLL 每个应用线程一条，Server 记宿主应用；焦点在哪个会话，组句就属于谁。

mod info;

use qingjian_platform::protocol::SessionId;

pub(super) use self::info::SessionInfo;
use super::Router;

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
        // 能收到按键的就是前台应用：钩子还没报过前台时（Server 刚起、用户没切过窗口）靠它点亮状态条。
        self.set_foreground(session);
        if self.focused != Some(session) {
            self.reset_composition();
            self.focused = Some(session);
            let app = self.focused_app().map(str::to_owned);
            self.engine.set_application(app);
            let private = self.focused_private();
            self.engine.set_private(private);
        }
    }

    /// 记下某会话报来的中英模式。中英模式是每会话一份的，状态条只显示前台那一份。
    pub(super) fn set_mode(&mut self, session: SessionId, english: bool) {
        if let Some(info) = self.sessions.get_mut(&session) {
            info.english = Some(english);
        }
    }

    /// 某会话最近报来的中英模式；会话已关或还没报过为 `None`。
    pub(super) fn mode_of(&self, session: SessionId) -> Option<bool> {
        self.sessions.get(&session).and_then(|info| info.english)
    }

    /// 某会话切成了别的输入法：模式作废，状态条不再拿它当「青简在前台」。
    pub(super) fn clear_mode(&mut self, session: SessionId) {
        if let Some(info) = self.sessions.get_mut(&session) {
            info.english = None;
        }
    }

    /// 按前台窗口的归属线索认出哪个会话在前台。
    ///
    /// `hints` 是 `(线程 id, 进程 id)` 列表，按可信度从高到低排（前台窗口本身 → 它的根祖先 → 它的后代窗口），
    /// 由 [`crate::ui`] 的 `EVENT_SYSTEM_FOREGROUND` 钩子收集。先拿线程 id 逐个对——TSF 按线程激活，
    /// 线程号唯一确定一个会话；都对不上再退到进程 id（宿主把编辑框放在另一条线程里时）。
    /// 老 DLL 不报 id（都是 0），一律对不上。
    pub(super) fn match_foreground(&self, hints: &[(u32, u32)]) -> Option<SessionId> {
        for &(tid, _) in hints {
            if tid != 0
                && let Some(session) = self.session_where(|info| info.tid == tid)
            {
                return Some(session);
            }
        }
        for &(_, pid) in hints {
            if pid != 0
                && let Some(session) = self.session_where(|info| info.pid == pid)
            {
                return Some(session);
            }
        }
        None
    }

    /// 第一个满足条件的会话；正在组句的那个优先（同一进程里可以有多条 TSF 线程，各开一个会话）。
    fn session_where(&self, matches: impl Fn(&SessionInfo) -> bool) -> Option<SessionId> {
        if let Some(focused) = self.focused
            && self.sessions.get(&focused).is_some_and(&matches)
        {
            return Some(focused);
        }
        self.sessions
            .iter()
            .find(|(_, info)| matches(info))
            .map(|(session, _)| *session)
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
