//! 一套配色。缺省两套取自 macOS 系统语义色在 sRGB 下的实测值（label 0.847、secondaryLabel 0.498…）。
//!
//! 主题色（[`Accent`]）只换三处：品牌色 [`Palette::accent`]、高亮底色 [`Palette::highlight`] 与
//! 悬停底色 [`Palette::hover`]。语义色不跟着变 —— 生词的橙、译文与序号的灰照旧。

use crate::color::Color;

/// 主题色：整体强调色取哪一支。取值与配置 `[general] accent` 一一对应，由壳负责映射。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Accent {
    /// 青简绿。
    #[default]
    Qingjian,

    /// 经典蓝。
    Classic,
}

impl Accent {
    /// 品牌色：状态条上「中 / 英」「，。」这类强调格与云朵图标。
    /// 浅底上要更深才压得住白，深底上要更亮才读得出来。
    const fn brand(self, dark: bool) -> Color {
        match (self, dark) {
            (Self::Qingjian, false) => Color::rgb(94, 140, 62),
            (Self::Qingjian, true) => Color::rgb(176, 206, 125),
            (Self::Classic, false) => Color::rgb(0, 122, 255),
            (Self::Classic, true) => Color::rgb(90, 168, 255),
        }
    }

    /// 高亮底色：当前候选那一行、状态条上鼠标停住的那一格。
    const fn highlight(self, dark: bool) -> Color {
        match (self, dark) {
            (Self::Qingjian, false) => Color::rgba(176, 206, 125, 127),
            (Self::Qingjian, true) => Color::rgba(36, 76, 36, 255),
            (Self::Classic, false) => Color::rgba(0, 122, 255, 41),
            (Self::Classic, true) => Color::rgba(0, 122, 255, 41),
        }
    }

    /// 悬停底色，比 [`Self::highlight`] 淡一档，两者同时出现也分得清。
    const fn hover(self, dark: bool) -> Color {
        match (self, dark) {
            (Self::Qingjian, false) => Color::rgba(176, 206, 125, 20),
            // 深底上同样的 alpha 看不太出来，比浅色的稍重一点。
            (Self::Qingjian, true) => Color::rgba(176, 206, 125, 32),
            (Self::Classic, false) => Color::rgba(0, 122, 255, 20),
            (Self::Classic, true) => Color::rgba(0, 122, 255, 32),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// 候选词。
    pub text: Color,

    /// 译文。
    pub gloss: Color,

    /// 词性，比译文更浅。
    pub pos: Color,

    /// 生词译文：比普通译文醒目，看熟了就回到译文色。
    pub fresh: Color,

    /// 序号。
    pub index: Color,

    /// 品牌色：状态条上的强调格与云朵图标（主题色的那一支，见 [`Accent`]）。
    pub accent: Color,

    /// 窗口背景。
    pub background: Color,

    /// 当前候选的高亮底色。
    pub highlight: Color,

    /// 鼠标悬停那一行的底色：同一支色淡一档，与键盘高亮同时出现也分得清。
    pub hover: Color,
}

impl Palette {
    /// 浅色，主题色用缺省的青简绿。
    pub const fn light() -> Self {
        Self::light_with(Accent::Qingjian)
    }

    /// 深色，主题色用缺省的青简绿。
    pub const fn dark() -> Self {
        Self::dark_with(Accent::Qingjian)
    }

    /// 浅色 + 指定主题色。
    pub const fn light_with(accent: Accent) -> Self {
        Self {
            text: Color::gray(0, 216),
            gloss: Color::gray(0, 127),
            pos: Color::gray(0, 66),
            fresh: Color::rgb(255, 141, 40),
            index: Color::gray(0, 66),
            accent: accent.brand(false),
            background: Color::rgb(255, 255, 255),
            highlight: accent.highlight(false),
            hover: accent.hover(false),
        }
    }

    /// 深色 + 指定主题色。
    pub const fn dark_with(accent: Accent) -> Self {
        Self {
            text: Color::gray(255, 216),
            gloss: Color::gray(255, 140),
            pos: Color::gray(255, 63),
            fresh: Color::rgb(255, 146, 48),
            index: Color::gray(255, 63),
            accent: accent.brand(true),
            background: Color::rgb(30, 30, 30),
            highlight: accent.highlight(true),
            hover: accent.hover(true),
        }
    }
}
