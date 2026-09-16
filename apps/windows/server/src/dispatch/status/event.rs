/// 用户在状态条上做的事，UI 线程发回 Router（打开设置不经过 Router，UI 线程自己起进程）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusEvent {
    /// 点了「中 / 英」格：翻转模式。
    ToggleMode,

    /// 点了「，。」格：翻转当前模式的全角标点（中英各记一份）。
    TogglePunctuation,

    /// 点了「☁」格：开 / 关在线联想。隐私开关：写回 `[predict] enabled` 并立刻换掉 Predictor，
    /// 关着时不再向服务商发任何请求（云联想与释义兜底一起关）。
    ToggleCloud,

    /// 拖动结束，内容左上角的新位置（物理像素）。
    Moved(i32, i32),
}
