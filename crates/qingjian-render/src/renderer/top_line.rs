//! 顶部拼音行：各段按样式画、自己画光标、右侧整句补全或临时状态。

use super::{CARET_WIDTH, Metrics, Rect, Renderer, SENTENCE_GAP};
use crate::canvas::Canvas;
use crate::frame::{Frame, Hover, Preedit, PreeditStyle};

impl Renderer {
    /// 顶部拼音行（含右侧整句补全）需要的宽高；没有这一行时都是 0。
    pub(super) fn top_line_size(&mut self, frame: &Frame, m: &Metrics) -> (f32, f32) {
        if !frame.has_top_line() {
            return (0.0, 0.0);
        }
        let style = m.annotation_style(m.theme.colors.gloss);
        let line_height = style.line_height;
        let mut width = 0.0;
        if let Some(preedit) = &frame.preedit {
            width += self.measure(&preedit.text(), &style).width + m.px(CARET_WIDTH);
        }
        if let Some(tail) = frame.trailing() {
            if frame.preedit.is_some() {
                width += m.px(SENTENCE_GAP);
            }
            if tail.cloud {
                width += m.cloud_width();
            }
            width += self.measure(tail.text, &style).width;
        }
        (width, line_height + m.row_padding() * 2.0)
    }

    /// 返回占用高度与整句补全的矩形（壳按它做点选）。`left` 是内容区左边。
    pub(super) fn draw_top_line(
        &mut self,
        canvas: &mut Canvas,
        frame: &Frame,
        m: &Metrics,
        left: f32,
        y: f32,
    ) -> (f32, Option<Rect>) {
        if !frame.has_top_line() {
            return (0.0, None);
        }
        let line_height = m.px(m.theme.annotation_font.line_height);
        let top = y + m.row_padding();
        let mut x = left + m.padding();
        if let Some(preedit) = &frame.preedit {
            x += self.draw_preedit(canvas, m, preedit, x, top, line_height);
            if frame.trailing().is_some() {
                x += m.px(SENTENCE_GAP);
            }
        }
        // 整句补全：云朵 + 句子，颜色与本地候选区分；临时状态与等待占位灰字、不给命中小
        let mut trailing = None;
        if let Some(tail) = frame.trailing() {
            let start_x = x;
            let color = if tail.cloud {
                x += self.draw_cloud(canvas, m, x, top, line_height);
                m.theme.colors.accent
            } else {
                m.theme.colors.gloss
            };
            let style = m.annotation_style(color);
            let text_width = self.measure(tail.text, &style).width;
            // 只有真整句可以点（临时状态、等待占位都不行）；可点的范围与悬停底色同一块，
            // 四周留一点，点起来不必像素级对准文字。
            if tail.pointable {
                let band = Rect {
                    x: start_x - m.padding() / 2.0,
                    y: top - m.row_padding() / 2.0,
                    width: (x + text_width) - start_x + m.padding(),
                    height: line_height + m.row_padding(),
                };
                if frame.hovered == Some(Hover::Trailing) {
                    let hover = m.theme.colors.hover;
                    self.fill_band(canvas, m, hover, band);
                }
                trailing = Some(band);
            }
            self.draw_text(canvas, tail.text, &style, x, top);
        }
        (line_height + m.row_padding() * 2.0, trailing)
    }

    /// 画拼音行的各段与光标，返回占用宽度（含光标）。
    fn draw_preedit(
        &mut self,
        canvas: &mut Canvas,
        m: &Metrics,
        preedit: &Preedit,
        x: f32,
        top: f32,
        line_height: f32,
    ) -> f32 {
        let mut cursor_x = x;
        for segment in &preedit.segments {
            let style = match segment.style {
                PreeditStyle::Typed => m.annotation_style(m.theme.colors.gloss),
                PreeditStyle::Rest => m.annotation_style(m.theme.colors.pos),
                PreeditStyle::Struck => m.annotation_style(m.theme.colors.pos).struck(),
            };
            cursor_x += self.draw_text(canvas, &segment.text, &style, cursor_x, top);
        }
        let measure_style = m.annotation_style(m.theme.colors.gloss);
        let caret_x = x + self.measure(&preedit.before_cursor(), &measure_style).width;
        canvas.fill_rect(
            caret_x,
            top,
            m.px(CARET_WIDTH),
            line_height,
            m.theme.colors.text,
        );
        cursor_x - x + m.px(CARET_WIDTH)
    }
}
