//! 会话信息。

/// 一个活跃会话在 Server 侧记下的信息，开会话时由 DLL 报来。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionInfo {
    /// 宿主应用的 exe 文件名（如 `Code.exe`），查 `[apps]` 用；取不到为 `None`。
    pub(crate) app: Option<String>,

    /// 该会话当前落在私密输入框里（DLL 随 `ClientMessage::Privacy` 报来）；焦点切回来时按它重设 Engine。
    pub(crate) private: bool,

    /// 宿主进程 id；老 DLL 不报，是 0（对不上任何窗口）。
    pub(crate) pid: u32,

    /// 宿主线程 id（TSF 在这条线程上激活）；老 DLL 不报，是 0。
    pub(crate) tid: u32,

    /// 该会话最近报来的中英模式（`true` 英文）；还没报过、或已切成别的输入法时为 `None`。
    /// 中英模式是**每会话一份**的：状态条显示前台会话那一份，后台应用报来的只记不显。
    pub(crate) english: Option<bool>,

    /// 该会话最近一次报来的光标**前**文（`ClientMessage::Surrounding`）；读到空就是 `None`。
    /// **按会话各记一份**：以前这是 `Router` 上的全局一份、谁最后报谁的 —— 别的窗口打字会把正在
    /// 输入那个窗口的上下文顶掉（2026-09-18 诊断日志实证：A 应用复现期间，B 应用每敲一个字都在覆盖它）。
    pub(crate) surrounding_before: Option<String>,

    /// 该会话最近一次报来的光标**后**文；含义同上。
    pub(crate) surrounding_after: Option<String>,

    /// 该会话最近报来的 Caps Lock 状态（`ModeChanged` 一起带过来）：状态条的模式格前面加个「A」。
    /// 老 DLL 不带这个字段（升级后没重启的应用），按灭处理。
    pub(crate) caps: bool,
}
