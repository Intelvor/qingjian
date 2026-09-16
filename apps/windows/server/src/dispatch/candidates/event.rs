/// 用户在候选窗口上做的事，UI 线程发回 Router（做法同状态条的 [`StatusEvent`](super::super::StatusEvent)）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateEvent {
    /// 点了本页第 `row` 行（页内序号，从 0 起）：选它上屏。
    /// 报页内序号而不是全局下标：候选窗手里只有一帧，`page_size` 在 Router 上。
    Pick(usize),

    /// 点了顶部的整句补全（云联想给的整段结果）：接受它上屏，与 Tab 同一条路。
    PickSentence,
}
