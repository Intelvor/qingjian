use qingjian_platform::protocol::{Frame, InputSettings, KeyOutcome};

/// Server 对一次「同步中英模式」轮询的答复。
pub struct ModeSyncReply {
    /// `Some(true)` 切英文、`Some(false)` 切中文；`None` 没有待处理的切换。
    pub english: Option<bool>,

    /// 当前的按键行为设置（切换键、内置英文模式）。每一拍都带，配置改了靠它生效——
    /// DLL 不读配置文件，`%APPDATA%\Qingjian` 对 AppContainer 里的商店应用本来也读不到。
    pub input: InputSettings,
}

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
    /// 点选发生在没按键的时候，而传输是一问一答，只能攒到这一拍带回（见 `ServerMessage::Update`）。
    pub commit: Option<String>,
}
