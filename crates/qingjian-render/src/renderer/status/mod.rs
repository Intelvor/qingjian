//! 悬浮状态条（Windows）：几格并排的小条 `[中 / 英][，。/ ,.][⚙]`，每格文字居中、格间一条细线，圆角背景加阴影。
//! macOS 用菜单栏状态项，没有这一块。

mod cell;
mod rendered;

pub use cell::StatusCell;
pub use rendered::RenderedStatus;

use super::{CLOUD_SIZE, Metrics, Rect, Rendered, Renderer};
use crate::canvas::Canvas;
use crate::cloud::draw_cloud;
use crate::error::RenderError;
use crate::gear::draw_gear;
use crate::shadow::Shadow;
use crate::theme::Theme;

/// 齿轮图标边长（点）。
const GEAR_SIZE: f32 = 15.0;

/// 格间细线的宽度（点）。
const SEPARATOR_WIDTH: f32 = 1.0;

impl Renderer {
    /// 画状态条。每格宽 = 内容宽 + 两侧内边距，高 = 候选词行高 + 内边距；
    /// 返回位图与各格的可点矩形（供悬停高亮与点击命中）。`hovered` 是那格的序号。
    pub fn render_status(
        &mut self,
        cells: &[StatusCell],
        theme: &Theme,
        scale: f32,
        shadow: Option<&Shadow>,
        hovered: Option<usize>,
    ) -> Result<RenderedStatus, RenderError> {
        let metrics = Metrics { theme, scale };
        let padding = metrics.padding();
        let line_height = metrics.px(theme.text_font.line_height);
        let mut widths: Vec<f32> = cells
            .iter()
            .map(|cell| self.status_cell_width(cell, &metrics) + padding * 2.0)
            .collect();
        let total: f32 = widths.iter().sum();
        let content_width = total.ceil();
        // 取整多出来的零头给最后一格，让最后一格的右边界正好是内容宽
        if let Some(last) = widths.last_mut() {
            *last += content_width - total;
        }
        let content_height = (line_height + padding).ceil();
        let margin = shadow.map_or(0.0, |s| metrics.px(s.margin()));
        let width = (content_width + margin * 2.0).ceil();
        let height = (content_height + margin * 2.0).ceil();
        let mut canvas = Canvas::new(width as u32, height as u32)?;
        let radius = metrics.corner_radius();
        if let Some(shadow) = shadow
            && let Some(content) =
                tiny_skia::Rect::from_xywh(margin, margin, content_width, content_height)
        {
            shadow.paint(&mut canvas, content, radius, scale);
        }
        canvas.fill_round_rect(
            margin,
            margin,
            content_width,
            content_height,
            radius,
            theme.colors.background,
        );
        let inset = padding / 2.0;
        let mut x = margin;
        let mut rects = Vec::with_capacity(cells.len());
        for (i, (cell, width)) in cells.iter().zip(&widths).enumerate() {
            if i > 0 {
                canvas.fill_rect(
                    x,
                    margin + inset,
                    metrics.px(SEPARATOR_WIDTH),
                    content_height - inset * 2.0,
                    theme.colors.pos,
                );
            }
            // 可点范围与悬停底色同一块：格子左右各留半个内边距（让开分隔线），上下也留一点。
            let band = Rect {
                x: x - margin + inset,
                y: inset / 2.0,
                width: width - inset * 2.0,
                height: content_height - inset,
            };
            if Some(i) == hovered {
                let hover = theme.colors.hover;
                let at = Rect {
                    x: band.x + margin,
                    y: band.y + margin,
                    ..band
                };
                self.fill_band(&mut canvas, &metrics, hover, at);
            }
            let slot = (x, margin, *width, content_height);
            self.draw_status_cell(&mut canvas, cell, &metrics, slot);
            x += width;
            rects.push(band);
        }
        Ok(RenderedStatus {
            rendered: Rendered {
                pixmap: canvas.into_pixmap(),
                content_x: margin as u32,
                content_y: margin as u32,
                content_width: content_width as u32,
                content_height: content_height as u32,
                scale,
                rows: Vec::new(),
                sentence: None,
            },
            cell_rects: rects,
        })
    }

    /// 一格内容的宽度（像素，不含内边距）。
    ///
    /// 有**宽度基准串**（[`StatusCell::text_with_width`]）就按基准量 —— 文字在中 / 英 / 注、
    /// `，。` / `,.` 之间切换时状态条长度不变；画的时候仍按实际文字居中。
    fn status_cell_width(&mut self, cell: &StatusCell, m: &Metrics) -> f32 {
        match cell {
            StatusCell::Text { text, width_of, .. } => {
                let measured = width_of.as_deref().unwrap_or(text);
                self.measure(measured, &m.text_style()).width
            }
            StatusCell::Cloud { .. } => m.px(CLOUD_SIZE),
            StatusCell::Gear => m.px(GEAR_SIZE),
        }
    }

    /// 在 `slot = (x, y, 宽, 高)` 的格子里居中画一格。
    fn draw_status_cell(
        &mut self,
        canvas: &mut Canvas,
        cell: &StatusCell,
        m: &Metrics,
        slot: (f32, f32, f32, f32),
    ) {
        let (x, y, width, height) = slot;
        match cell {
            StatusCell::Text {
                text, emphasized, ..
            } => {
                let color = if *emphasized {
                    m.theme.colors.accent
                } else {
                    m.theme.colors.gloss
                };
                let style = m.style(m.theme.text_font, color);
                let size = self.measure(text, &style);
                let left = x + (width - size.width) / 2.0;
                let top = y + (height - size.height) / 2.0;
                self.draw_text(canvas, text, &style, left, top);
            }
            StatusCell::Cloud { emphasized } => {
                let size = m.px(CLOUD_SIZE);
                let color = if *emphasized {
                    m.theme.colors.accent
                } else {
                    m.theme.colors.gloss
                };
                draw_cloud(
                    canvas,
                    x + (width - size) / 2.0,
                    y + (height - size) / 2.0,
                    size,
                    color,
                );
            }
            StatusCell::Gear => {
                let size = m.px(GEAR_SIZE);
                draw_gear(
                    canvas,
                    x + (width - size) / 2.0,
                    y + (height - size) / 2.0,
                    size,
                    m.theme.colors.gloss,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StatusCell;
    use crate::fonts::FontLibrary;
    use crate::renderer::Renderer;
    use crate::shadow::Shadow;
    use crate::theme::Theme;

    #[test]
    fn cell_rects_increase_and_clear_both_edges() {
        // 没有系统字体的环境（CI 容器）跳过
        let Ok(library) = FontLibrary::system("zh-CN") else {
            return;
        };
        let mut renderer = Renderer::new(library);
        let cells = [
            StatusCell::text("中 · 小鹤", true),
            StatusCell::text(",.", false),
            StatusCell::cloud(true),
            StatusCell::Gear,
        ];
        let out = renderer
            .render_status(
                &cells,
                &Theme::light(),
                2.0,
                Some(&Shadow::mac_panel()),
                None,
            )
            .unwrap();
        assert_eq!(out.cell_rects.len(), 4);
        let (first, last) = (out.cell_rects[0], *out.cell_rects.last().unwrap());
        // 命中矩形两端都让开了边（那圈留白点不着），相邻两格之间也让开——留的就是那条分隔线。
        assert!(first.x > 0.0);
        assert!(last.x + last.width < out.rendered.content_width as f32);
        assert!(
            out.cell_rects
                .windows(2)
                .all(|pair| pair[0].x + pair[0].width < pair[1].x)
        );
        assert!(out.rendered.pixmap.width() > out.rendered.content_width);
        assert!(out.rendered.content_x > 0);
    }

    /// 宽度基准串：全角 ↔ 半角、中 ↔ 英 ↔ 注这类来回切**不改变整条长度**；不给基准才会伸缩。
    #[test]
    fn width_reference_keeps_the_bar_length_stable() {
        // 没有系统字体的环境（CI 容器）跳过
        let Ok(library) = FontLibrary::system("zh-CN") else {
            return;
        };
        let mut renderer = Renderer::new(library);
        let theme = Theme::light();
        let width = |renderer: &mut Renderer, cell: StatusCell| {
            renderer
                .render_status(&[cell], &theme, 1.0, None, None)
                .unwrap()
                .rendered
                .content_width
        };

        // 给了基准：两种写法一样宽（标点格按「，。」量、模式格按「A 中」量）。
        assert_eq!(
            width(
                &mut renderer,
                StatusCell::text_with_width("，。", true, "，。")
            ),
            width(
                &mut renderer,
                StatusCell::text_with_width(",.", false, "，。")
            ),
            "全 / 半角切换不该改变状态条长度"
        );
        assert_eq!(
            width(
                &mut renderer,
                StatusCell::text_with_width("中", true, "A 中")
            ),
            width(
                &mut renderer,
                StatusCell::text_with_width("A 英", true, "A 中")
            ),
            "Caps 亮灭、中英切换都不该改变长度"
        );

        // 不给基准（旧行为）：半角比全角窄 —— 这正是之前「点一下长度就跳」的原因。
        assert!(
            width(&mut renderer, StatusCell::text("，。", true))
                > width(&mut renderer, StatusCell::text(",.", false))
        );
    }
}
