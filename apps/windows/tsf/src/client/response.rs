use qingjian_platform::protocol::{Frame, KeyOutcome};

/// Server 对一次按键的处理结果。
pub struct KeyResponse {
    /// 吃掉还是放行给应用。
    pub outcome: KeyOutcome,

    /// 本次要立即上屏到文档的文本。
    pub commit: Option<String>,

    /// 处理后的组句状态（preedit + 候选）；空帧表示收起候选窗口。
    pub frame: Frame,
}

/// Server 对一次轮询的答复。
pub struct PollReply {
    /// 当前组句状态；空帧表示组句已经结束。
    pub frame: Frame,

    /// 用户在候选窗口点了候选：Server 已经把这个词选上，本次要立即上屏的文本。
    /// 点选发生在没按键的时候，只能攒到轮询这一拍带回（见 `[ServerMessage::Update]`）。
    pub commit: Option<String>,
}
