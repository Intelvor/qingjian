//! 悬浮状态条：中英模式只在 DLL 侧，DLL 用 `ModeChanged` 推来（激活 / 切换时），Server **按会话各记一份**。
//! 状态条显示的是**前台会话**那一份 —— 前台是谁由 [`crate::ui`] 的 `EVENT_SYSTEM_FOREGROUND` 钩子认定
//! （见 [`Router::handle_foreground`]），所以切到别的应用时中 / 英跟着变，后台应用报来的模式也带不跑它。
//! 前台窗口不属于任何会话（那个应用没装青简 / 用的是别的输入法）就收起；切成别的输入法（`ImeSwitched`）
//! 与会话关闭（应用退出）同样没得显示。
//!
//! 状态条上的点击经 [`StatusEvent`] 回到这里：切模式记成 `pending_mode` 等**前台** DLL 用 `SyncMode` 来取，
//! 切标点 / 拖动写回配置文件（热加载会再读回来）。

mod event;
mod sink;
mod view;

use qingjian_platform::Config;
use qingjian_platform::protocol::SessionId;

pub use self::event::StatusEvent;
pub use self::sink::{NoopStatusSink, StatusSink};
pub use self::view::StatusView;
use super::Router;

impl Router {
    /// DLL 报来某会话的中英模式：记在那个会话名下，它是前台会话时状态条才跟着变。
    pub(super) fn handle_mode_changed(&mut self, session: SessionId, english: bool) {
        self.set_mode(session, english);
        match self.foreground {
            // 后台应用也会报模式（配置改了、激活了）：记下就行，别把状态条带到别的应用去 ——
            // 那正是「切应用后中 / 英乱跳」的老毛病。
            Some(foreground) if foreground != session => {
                tracing::debug!(?session, ?foreground, "后台会话报来的中英模式，只记不显");
                return;
            }
            // 还没有前台线索（Server 刚起、钩子认不出这个宿主）就把报模式的这个当前台，
            // 否则状态条要等用户敲第一个键才亮。老 DLL 不报宿主 id（升级后没重启的应用），
            // 前台永远认不出它，别让它占住这个位置。
            None if self.session_host(session).is_some() => self.foreground = Some(session),
            _ => {}
        }
        self.reconcile_status();
    }

    /// 某会话切成了别的输入法：它的模式作废，是前台就把状态条收起。
    pub(super) fn handle_ime_switched(&mut self, session: SessionId) {
        self.clear_mode(session);
        self.drop_foreground(session);
    }

    /// 收到某会话的按键：它就是前台（钩子还没报过时靠这条点亮状态条）。没变就什么都不做，
    /// 所以每个按键都调也不会重画。
    pub(super) fn set_foreground(&mut self, session: SessionId) {
        self.switch_foreground(Some(session));
    }

    /// 某会话关了（应用退出 / 切走输入法）：它在前台就没得显示了，收起。
    pub(super) fn drop_foreground(&mut self, session: SessionId) {
        if self.foreground == Some(session) {
            self.switch_foreground(None);
        }
    }

    /// 系统报来前台窗口变了（[`crate::ui`] 的 WinEvent 钩子）：按归属线索认出是哪个会话，状态条跟着它走。
    ///
    /// `hints` 是 `(线程 id, 进程 id)`，按可信度从高到低排（前台窗口本身 → 它的根祖先 → 它的后代窗口）。
    /// 认不出来就收起状态条 —— 接着显示上一个应用的中英模式正是这条路要修的毛病。
    pub fn handle_foreground(&mut self, hints: Vec<(u32, u32)>) {
        let matched = self.match_foreground(&hints);
        match matched {
            Some(session) => tracing::debug!(?session, ?hints, "前台窗口归到这个会话"),
            None => tracing::debug!(?hints, "前台窗口不属于任何会话，状态条收起"),
        }
        self.switch_foreground(matched);
    }

    /// 换前台会话的唯一入口：状态条跟着走，上一个应用还没取走的切换请求作废（别带给新前台）。
    fn switch_foreground(&mut self, session: Option<SessionId>) {
        if self.foreground == session {
            return;
        }
        self.foreground = session;
        self.pending_mode = None;
        self.reconcile_status();
    }

    /// 状态条要显示的中英模式 = 前台会话最近报来的那一份；没有前台会话（或它还没报过）是 `None`。
    fn status_mode(&self) -> Option<bool> {
        self.foreground.and_then(|session| self.mode_of(session))
    }

    /// DLL 来取状态条上点出的目标模式；取走即清。**只给前台会话**：所有装了青简的应用都在轮询，
    /// 谁先来给谁就把模式切进别的应用里去了，用户在前台看不到任何变化。
    pub(super) fn take_pending_mode(&mut self, session: SessionId) -> Option<bool> {
        if self.foreground != Some(session) {
            return None;
        }
        self.pending_mode.take()
    }

    /// 状态条上的操作。
    pub fn handle_status_event(&mut self, event: StatusEvent) {
        match event {
            StatusEvent::ToggleMode => {
                // 关掉内置英文模式后这一格不切模式：DLL 那边也会拦（配置改了没切走再切回时两边都挡住）
                if !self.config.english_mode {
                    tracing::debug!("内置英文模式已关闭，状态条不切模式");
                    return;
                }
                // 状态条看得见就说明前台会话报过模式，两个 `else` 只是把这条前提写死。
                let Some(session) = self.foreground else {
                    return;
                };
                let Some(english) = self.mode_of(session) else {
                    return;
                };
                // 先把状态条翻过来，前台 DLL 取走后回报 ModeChanged 再对一次账。
                self.pending_mode = Some(!english);
                self.set_mode(session, !english);
                tracing::debug!(english = !english, "状态条：请求切换中英模式");
            }
            StatusEvent::TogglePunctuation => {
                // 中英各记一份，切的是当前模式那份；还没报过模式时按中文算。
                let english = self.status_mode() == Some(true);
                let full_width = !self.full_width_for(english);
                let key = if english {
                    self.config.english_full_width = full_width;
                    "english_full_width_punctuation"
                } else {
                    self.config.full_width = full_width;
                    "full_width_punctuation"
                };
                tracing::debug!(english, full_width, "状态条：切换全角标点");
                self.persist("general", key, full_width);
            }
            StatusEvent::ToggleCloud => {
                // 隐私开关：翻转 `[predict] enabled`、写回配置文件，并**立刻**换掉 Predictor
                //（关着时连释义兜底一起停，不再向服务商发任何请求），不等热加载。
                let enabled = !self.predict.enabled;
                self.predict.enabled = enabled;
                tracing::info!(enabled, "状态条：切换在线联想");
                self.persist("predict", "enabled", enabled);
                if !enabled {
                    // 在飞的作废、回来的结果不认，手里那段整句也一并作废。
                    self.cancel_prediction();
                    self.drop_sentence();
                }
                super::reload::attach_cloud(&mut self.engine, &self.predict);
            }
            StatusEvent::Moved(x, y) => {
                self.config.status_pos = Some((x, y));
                self.persist("status_bar", "x", i64::from(x));
                self.persist("status_bar", "y", i64::from(y));
            }
        }
        self.reconcile_status();
    }

    /// 写回配置文件一个键；没有配置路径（测试）就只改内存。
    fn persist(&self, section: &str, key: &str, value: impl Into<toml_edit::Value>) {
        let Some(path) = self.config_path() else {
            return;
        };
        if let Err(error) = Config::set_value(path, section, key, value) {
            tracing::warn!(%error, section, key, "写回配置失败");
        }
    }

    /// 当前模式下标点转不转全角：中英各一份配置。
    pub(super) fn full_width_for(&self, english: bool) -> bool {
        if english {
            self.config.english_full_width
        } else {
            self.config.full_width
        }
    }

    /// 开着且认得出前台会话的模式就显示，否则收起。热加载后也调一次。
    pub(super) fn reconcile_status(&mut self) {
        match self.status_mode() {
            Some(english) if self.config.status_enabled => {
                self.status.show_status(StatusView {
                    english,
                    zhuyin: self.config.zhuyin,
                    scheme: self
                        .config
                        .shuangpin
                        .map(|scheme| scheme.label().to_owned()),
                    full_width: self.full_width_for(english),
                    cloud: self.predict.enabled,
                    theme: self.config.theme,
                    anchor: self.config.status_pos,
                });
            }
            _ => self.status.hide_status(),
        }
    }
}
