//! 一次按键对组句的影响。

/// 一次按键对组句的影响。
pub(crate) enum Effect {
    /// 缓冲变了，要重建 [`Composed`](super::super::Composed)；带本次要上屏的文本。
    Changed(Option<String>),

    /// 只挪了高亮 / 翻页。
    Navigated,

    /// 键吃掉了，但缓冲没变也不用导航：组句状态另有变化（按 Tab 现请了一次整句），
    /// 重画一帧让等待提示出来即可，不必重建 [`Composed`](super::super::Composed)。
    Waiting,

    /// 不吃，交还应用。
    Passthrough,
}
