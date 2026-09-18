use qingjian_platform::ThemeMode;

/// 状态条一次要显示的内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusView {
    /// 英文模式（`false` 中文）。
    pub english: bool,

    /// 是否啟用大千注音。
    pub zhuyin: bool,

    /// Caps Lock 亮着（大写锁定）：模式格前面加一个「A」。
    pub caps: bool,

    /// 当前模式的全角标点开着（中英各记一份配置）；关着时格子显示 `,.` 画成灰的。
    pub full_width: bool,

    /// 在线联想开着（`[predict] enabled`）：云朵格开着画品牌色、关着画灰的。
    pub cloud: bool,

    /// 外观模式。
    pub theme: ThemeMode,

    /// 配置里记住的内容左上角物理像素；`None` 首次按屏幕右下角摆。
    pub anchor: Option<(i32, i32)>,
}
