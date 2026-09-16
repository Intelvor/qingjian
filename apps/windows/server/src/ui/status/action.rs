//! 状态条一格的动作。

/// 状态条上一格点下去做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusAction {
    /// 「中 / 英」：切模式。
    ToggleMode,

    /// 「，。/ ,.」：切全角标点。
    TogglePunctuation,

    /// 「☁」：开 / 关在线联想（隐私开关，写回 `[predict] enabled` 并立刻生效）。
    ToggleCloud,

    /// 齿轮：打开设置程序（UI 线程直接起进程，不经 Router）。
    OpenSettings,
}
