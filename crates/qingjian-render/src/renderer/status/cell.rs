//! 状态条的一格。

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusCell {
    /// 一段文字；`emphasized` 用品牌色（当前模式、生效中的全角标点），否则用译文的灰。
    ///
    /// `width_of` 是**宽度基准串**：有些格的文字会在几个固定写法之间来回换（`，。` ↔ `,.`、
    /// 中 / 英 / 注），按实际文字量宽会让整条状态条跟着伸缩。给了基准就**按基准量宽、按实际
    /// 文字居中画** —— 长度在同一配置下恒定，长的写法（双拼方案名之类）照样放得下。
    Text {
        text: String,
        emphasized: bool,
        width_of: Option<String>,
    },

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
            width_of: None,
        }
    }

    /// 同 [`Self::text`]，但宽度按 `reference` 量：文字在几种写法间切换时状态条长度不变。
    pub fn text_with_width(
        text: impl Into<String>,
        emphasized: bool,
        reference: impl Into<String>,
    ) -> Self {
        Self::Text {
            text: text.into(),
            emphasized,
            width_of: Some(reference.into()),
        }
    }

    pub fn cloud(emphasized: bool) -> Self {
        Self::Cloud { emphasized }
    }
}
