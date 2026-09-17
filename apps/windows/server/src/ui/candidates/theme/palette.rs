//! 浅色 / 深色配色。
//!
//! GDI 没有 alpha，主题色（品牌色与高亮 / 悬停底色）按各自的不透明度预混到背景上，写成不透明值。

use windows::Win32::Foundation::COLORREF;

use super::rgb;
use qingjian_render::Accent;

/// 浅色 / 深色 + 主题色各一套。
pub(super) struct Palette {
    pub(super) text_color: COLORREF,
    pub(super) gloss_color: COLORREF,
    pub(super) pos_color: COLORREF,
    pub(super) fresh_color: COLORREF,
    pub(super) index_color: COLORREF,

    /// 品牌色：状态条上的强调格与云朵图标。
    pub(super) accent_color: COLORREF,

    pub(super) background: COLORREF,
    pub(super) highlight: COLORREF,

    /// 鼠标悬停那一格的底色：同一支色淡一档，与键盘高亮同时出现也分得清。
    pub(super) hover: COLORREF,
}

impl Palette {
    /// 贴近 mac light：label / secondary / tertiary label、systemOrange。`accent` 决定品牌色与高亮 / 悬停。
    pub(super) fn light(accent: Accent) -> Self {
        let (accent_color, highlight, hover) = match accent {
            Accent::Qingjian => (
                rgb(0x5e, 0x8c, 0x3e),
                // 青简绿 @50% 叠在浅背景上。
                rgb(0xd4, 0xe3, 0xbb),
                rgb(0xf2, 0xf5, 0xee),
            ),
            Accent::Classic => (
                rgb(0x00, 0x7a, 0xff),
                // sRGB(0,0.48,1.0) @16% 叠在浅背景上。
                rgb(0xcf, 0xe4, 0xf9),
                rgb(0xe4, 0xee, 0xff),
            ),
        };
        Self {
            text_color: rgb(0x1d, 0x1d, 0x1f),
            gloss_color: rgb(0x6b, 0x6b, 0x70),
            pos_color: rgb(0xa0, 0xa0, 0xa6),
            fresh_color: rgb(0xff, 0x95, 0x00),
            index_color: rgb(0xa0, 0xa0, 0xa6),
            accent_color,
            background: rgb(0xf8, 0xf8, 0xf8),
            highlight,
            hover,
        }
    }

    /// 贴近 mac dark。深底上的主题色要更亮、底色要更重才看得出来。
    pub(super) fn dark(accent: Accent) -> Self {
        let (accent_color, highlight, hover) = match accent {
            Accent::Qingjian => (
                rgb(0xb0, 0xce, 0x7d),
                rgb(0x24, 0x4c, 0x24),
                rgb(0x3b, 0x3f, 0x34),
            ),
            Accent::Classic => (
                rgb(0x5a, 0xa8, 0xff),
                rgb(0x2f, 0x4d, 0x72),
                rgb(0x24, 0x35, 0x4a),
            ),
        };
        Self {
            text_color: rgb(0xf5, 0xf5, 0xf7),
            gloss_color: rgb(0xae, 0xae, 0xb2),
            pos_color: rgb(0x8e, 0x8e, 0x93),
            fresh_color: rgb(0xff, 0x9f, 0x0a),
            index_color: rgb(0x8e, 0x8e, 0x93),
            accent_color,
            background: rgb(0x2a, 0x2a, 0x2c),
            highlight,
            hover,
        }
    }
}
