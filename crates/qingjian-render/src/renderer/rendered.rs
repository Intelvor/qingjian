//! 渲染结果：位图加内容区（阴影边之外的那块）的位置，以及候选行与整句补全的矩形（供壳做鼠标命中）。

use tiny_skia::Pixmap;

/// 一块矩形（像素），坐标相对内容区左上角。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,

    pub y: f32,

    pub width: f32,

    pub height: f32,
}

pub struct Rendered {
    /// 预乘 RGBA 位图，含阴影边。
    pub pixmap: Pixmap,

    /// 内容区左上角在位图里的像素坐标。
    pub content_x: u32,

    pub content_y: u32,

    /// 内容区像素宽高（窗口该有的大小）。
    pub content_width: u32,

    pub content_height: u32,

    /// 渲染用的倍数，壳把像素换回点用。
    pub scale: f32,

    /// 各候选行在内容区里的矩形，从上到下 / 从左到右；点选按它落进哪行。
    pub rows: Vec<Rect>,

    /// 顶部行整句补全的矩形；没有整句为 `None`。点选它等于接受这条整句。
    pub sentence: Option<Rect>,
}

impl Rendered {
    /// 内容区宽高换回点。
    pub fn content_size_points(&self) -> (f32, f32) {
        (
            self.content_width as f32 / self.scale,
            self.content_height as f32 / self.scale,
        )
    }
}
