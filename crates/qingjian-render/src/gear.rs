//! 状态条的齿轮：八齿外圈加一个中心孔，对应 Windows 端原先用的 ⚙ 字形（Segoe UI Symbol）。
//! 渲染器自己画是为了不受字体回退影响：Segoe UI Emoji 会把 U+2699 画成彩色。
//!
//! 本体填充、只把中心孔挖空——状态条上的图标才 15 pt，齿又要跟描边抢那点高度，
//! 线描画出来的齿会被自己的描边吃光，看着只剩一圈分离的小方块。

use std::f32::consts::TAU;

use tiny_skia::{BlendMode, PathBuilder, Pixmap};

use crate::canvas::Canvas;
use crate::color::Color;

/// 齿数。
const TEETH: usize = 8;

/// 中心孔半径相对边长的比例。
const HOLE_RATIO: f32 = 0.15;

/// 画在 `(x, y)` 为左上角、边长 `size` 像素的方块里。
pub(crate) fn draw_gear(canvas: &mut Canvas, x: f32, y: f32, size: f32, color: Color) {
    let side = size.ceil() as u32 + 1;
    let Some(icon) = Pixmap::new(side, side) else {
        return;
    };
    let mut layer = Canvas::from_pixmap(icon);
    if let Some(body) = gear_outline(size) {
        layer.fill_path(&body, color, BlendMode::SourceOver);
    }
    let center = size / 2.0;
    let mut hole = PathBuilder::new();
    hole.push_circle(center, center, size * HOLE_RATIO);
    if let Some(hole) = hole.finish() {
        layer.fill_path(&hole, Color::rgb(0, 0, 0), BlendMode::Clear);
    }
    let icon = layer.into_pixmap();
    canvas.blend_pixmap(x.round() as i32, y.round() as i32, &icon);
}

/// 齿轮外轮廓：齿顶与齿根交替的折线。
fn gear_outline(size: f32) -> Option<tiny_skia::Path> {
    let center = size / 2.0;
    let outer = size * 0.50;
    let inner = size * 0.35;
    // 每齿四个顶点：齿根起、齿顶起、齿顶止、齿根止；两侧取同一个角度，齿就是径向的，
    // 齿与齿之间留出的凹口比齿本身宽，一排齿才分得开。
    let step = TAU / TEETH as f32;
    let half = step * 0.21;
    let mut path = PathBuilder::new();
    for i in 0..TEETH {
        let mid = i as f32 * step;
        let points = [
            (mid - half, inner),
            (mid - half, outer),
            (mid + half, outer),
            (mid + half, inner),
        ];
        for (j, (angle, radius)) in points.iter().enumerate() {
            let px = center + radius * angle.cos();
            let py = center + radius * angle.sin();
            if i == 0 && j == 0 {
                path.move_to(px, py);
            } else {
                path.line_to(px, py);
            }
        }
    }
    path.close();
    path.finish()
}
