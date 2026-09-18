//! 状态条的一格。

use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Gdi::HFONT;

use super::StatusAction;

/// 一格：文字、字体、颜色、点下去做什么。
pub(super) struct CellSpec {
    pub(super) text: String,

    /// 宽度基准串：给了就按它量宽（文字在几种写法间切换时整条长度不变），仍按 `text` 居中画。
    pub(super) width_of: Option<String>,

    pub(super) font: HFONT,

    pub(super) color: COLORREF,

    pub(super) action: StatusAction,
}
