//! 状态条的一格。

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusCell {
    /// 一段文字；`emphasized` 用品牌色（当前模式、生效中的全角标点），否则用译文的灰。
    Text { text: String, emphasized: bool },

    /// 在线联想的云朵；`emphasized`（开着）用云朵色，关着用译文的灰。
    /// 自绘而不是排 `☁` 字形：那个码位会被 Segoe UI Emoji 接走，画成彩色的，不认我们给的颜色。
    Cloud { emphasized: bool },

    /// 打开设置的齿轮。
    Gear,
}

impl StatusCell {
    pub fn text(text: impl Into<String>, emphasized: bool) -> Self {
        Self::Text {
            text: text.into(),
            emphasized,
        }
    }

    pub fn cloud(emphasized: bool) -> Self {
        Self::Cloud { emphasized }
    }
}
