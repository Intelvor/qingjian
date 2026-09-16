//! 状态条的渲染结果：位图加各格的可点矩形。

use crate::renderer::{Rect, Rendered};

pub struct RenderedStatus {
    /// 位图与内容区位置。
    pub rendered: Rendered,

    /// 各格的可点矩形（相对内容区左上角，与画出的悬停底色同一块），从左到右；
    /// 点击与悬停都按它判断，不必壳自己再算内缩。
    pub cell_rects: Vec<Rect>,
}
