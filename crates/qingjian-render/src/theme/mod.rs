//! 主题：字体、颜色、间距。所有可视参数都在这里，单位是点；将来从 TOML 读。
//!
//! 视觉层级（产品决定）：候选词最深，译文稍浅，词性最浅，序号弱化。数值对齐 macOS 壳的 AppKit 实现；
//! 只有行内上下留白（`row_padding`）按 Windows 端的观感调宽过一档——候选行挨太紧不好认。

mod font_spec;
mod palette;

pub use font_spec::FontSpec;
pub use palette::Palette;

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// 候选词字体。
    pub text_font: FontSpec,

    /// 译文与词性字体。
    pub annotation_font: FontSpec,

    /// 序号字体。
    pub index_font: FontSpec,

    /// 配色。
    pub colors: Palette,

    /// 窗口内边距。
    pub padding: f32,

    /// 行内上下留白。
    pub row_padding: f32,

    /// 序号与候选词、候选词与译文之间的间距。
    pub column_gap: f32,

    /// 窗口与高亮条的圆角。
    pub corner_radius: f32,

    /// 最多显示几行。
    pub max_rows: usize,

    /// 文字抗锯齿覆盖率的 gamma：小于 1 笔画显粗。CoreText 对文字有一层类似的加深，深色背景上尤其明显，
    /// 线性混合出来的字会偏细；这个值按真机截图并排调。
    pub text_gamma: f32,
}

impl Theme {
    /// 浅色，对齐 macOS 系统外观；字号 16 点。
    pub fn light() -> Self {
        Self::with_palette(Palette::light(), 0.85, 16.0)
    }

    /// 深色，对齐 macOS 系统外观；字号 16 点。
    pub fn dark() -> Self {
        Self::with_palette(Palette::dark(), 0.75, 16.0)
    }

    /// 按给定字号复制一份主题，比例保持与 16 点基准一致。
    pub fn with_font_size(self, font_size: f32) -> Self {
        let scale = font_size / self.text_font.size;
        Self {
            text_font: self.text_font.scaled(scale),
            annotation_font: self.annotation_font.scaled(scale),
            index_font: self.index_font.scaled(scale),
            ..self
        }
    }

    fn with_palette(colors: Palette, text_gamma: f32, base_font_size: f32) -> Self {
        let scale = base_font_size / 16.0;
        Self {
            // 行高取 AppKit 系统字体在这几个字号下 NSAttributedString.size() 的高度
            text_font: FontSpec::new(16.0, 19.0).scaled(scale),
            annotation_font: FontSpec::new(12.0, 15.0).scaled(scale),
            index_font: FontSpec::new(11.0, 14.0).scaled(scale),
            colors,
            padding: 8.0,
            row_padding: 5.0,
            column_gap: 8.0,
            corner_radius: 8.0,
            max_rows: 9,
            text_gamma,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_size_scales_all_fonts_proportionally() {
        let base = Theme::light();
        assert_eq!(base.text_font.size, 16.0);
        assert_eq!(base.annotation_font.size, 12.0);
        assert_eq!(base.index_font.size, 11.0);

        let scaled = base.with_font_size(20.0);
        assert_eq!(scaled.text_font.size, 20.0);
        assert_eq!(scaled.annotation_font.size, 15.0);
        assert_eq!(scaled.index_font.size, 13.75);
        assert_eq!(scaled.text_font.line_height, 19.0 * 20.0 / 16.0);
    }
}
