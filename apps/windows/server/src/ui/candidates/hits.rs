//! 候选窗的鼠标命中：窗口过程按 HWND 查到它，把鼠标位置对到哪一行候选上（悬停高亮、点击选词）。
//!
//! 行的位置在画完一帧之后由渲染器带出来（`qingjian-render::Rendered::rows` / `sentence`，
//! 坐标相对内容区左上角），竖排按 y、横排按 x 判。内容区外那圈是阴影留白，不算命中。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::mem::size_of;
use std::rc::Rc;

use qingjian_render::{Hover, Rect};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use super::Inner;
use crate::dispatch::CandidateEvent;
use crate::ui::CandidateEvents;

thread_local! {
    /// 本线程活着的候选窗：HWND → 命中状态。窗口过程按 HWND 查，查不到（还没建 / 已析构）就忽略。
    static HITS: RefCell<HashMap<isize, Rc<Hits>>> = RefCell::new(HashMap::new());
}

/// 一帧里能点的地方：候选行的矩形与整句补全的矩形（都相对内容区左上角），
/// 外加内容区在客户区里的位置。窗口为了四周留出阴影，左上角比内容区多出那一圈，
/// 鼠标坐标得先减掉它才跟矩形落在同一套坐标系里——不减的话命中整体偏右下，
/// 竖排看着像「高亮跑到鼠标下面那一行」，整句那块只有一行高，直接就点不中了。
pub(super) struct Bands {
    pub(super) rows: Vec<Rect>,
    pub(super) sentence: Option<Rect>,

    /// 内容区在客户区里的位置与大小：鼠标落在它外面（阴影留白、窗口之外）一律不算命中，
    /// 高亮跟着收掉，不会留在窗口边上那圈没内容的地方。
    pub(super) content: Rect,
}

impl Default for Bands {
    /// 空的：一块零尺寸的内容区，什么也命不中（GDI 退路与窗口还没显示时用）。
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            sentence: None,
            content: Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
        }
    }
}

/// 绘制状态与窗口过程共享的命中状态。
pub(super) struct Hits {
    /// 绘制状态：悬停换行要照原样重贴一次。
    inner: Rc<Inner>,

    /// 这一帧里能点的地方。
    bands: RefCell<Bands>,

    /// 竖排（按 y 判行）还是横排（按 x 判行）。
    vertical: Cell<bool>,

    /// 鼠标悬停的地方；不在可点范围里为 `None`。
    hover: Cell<Option<Hover>>,

    /// 已登记 `WM_MOUSELEAVE`；鼠标每次移进窗口登记一次，收到离开后要重登记。
    tracking: Cell<bool>,

    /// 点选回给 Router。
    events: CandidateEvents,
}

impl Hits {
    /// 建好并登记到本线程的 HWND 表里。
    pub(super) fn attach(inner: Rc<Inner>, events: CandidateEvents) -> Rc<Self> {
        let hits = Rc::new(Self {
            inner,
            bands: RefCell::new(Bands::default()),
            vertical: Cell::new(true),
            hover: Cell::new(None),
            tracking: Cell::new(false),
            events,
        });
        HITS.with(|map| {
            map.borrow_mut()
                .insert(hits.inner.hwnd.0 as isize, hits.clone())
        });
        hits
    }

    /// 窗口析构时摘掉，免得窗口过程拿到已经不画的那个。
    pub(super) fn detach(hwnd: HWND) {
        HITS.with(|map| map.borrow_mut().remove(&(hwnd.0 as isize)));
    }

    /// 窗口过程按 HWND 查；查不到返回 `None`。
    pub(super) fn of(hwnd: HWND) -> Option<Rc<Self>> {
        HITS.with(|map| map.borrow().get(&(hwnd.0 as isize)).cloned())
    }

    /// 一帧画完：换上新算的可点范围。悬停按鼠标现在的位置重算——内容刷新时鼠标常常没动，
    /// 不清的话高亮会留在已经不存在的行上，清了又收不到新的 `WM_MOUSEMOVE`。
    pub(super) fn set_bands(&self, bands: Bands, vertical: bool) {
        *self.bands.borrow_mut() = bands;
        self.vertical.set(vertical);
        self.refresh_hover();
    }

    /// 窗口收起：悬停与离开跟踪都清掉（下次显示鼠标移进来重新算、重新登记）。
    pub(super) fn on_hide(&self) {
        self.tracking.set(false);
        self.update_hover(None);
    }

    /// 鼠标移到客户区 `(x, y)`。
    pub(super) fn on_mouse_move(&self, x: i32, y: i32) {
        self.track_leave();
        self.update_hover(self.target_at(x, y));
    }

    /// 鼠标离开窗口：收掉悬停高亮。
    pub(super) fn on_mouse_leave(&self) {
        self.tracking.set(false);
        self.update_hover(None);
    }

    /// 在客户区 `(x, y)` 按下左键：点中候选行就选它，点中整句补全就接受它。
    pub(super) fn on_click(&self, x: i32, y: i32) {
        match self.target_at(x, y) {
            Some(Hover::Row(row)) => (self.events)(CandidateEvent::Pick(row)),
            Some(Hover::Trailing) => (self.events)(CandidateEvent::PickSentence),
            None => {}
        }
    }

    fn update_hover(&self, target: Option<Hover>) {
        if target != self.hover.get() {
            self.hover.set(target);
            self.inner.repaint(target);
        }
    }

    /// 按鼠标当前位置重算悬停（窗口没动、鼠标也没动时收不到消息，得自己问一次）。
    fn refresh_hover(&self) {
        let mut point = POINT::default();
        if unsafe { GetCursorPos(&mut point) }.is_err()
            || !unsafe { ScreenToClient(self.inner.hwnd, &mut point) }.as_bool()
        {
            self.update_hover(None);
            return;
        }
        self.update_hover(self.target_at(point.x, point.y));
    }

    /// 客户区坐标底下是什么。内容区外那圈是阴影留白，不算命中。
    fn target_at(&self, x: i32, y: i32) -> Option<Hover> {
        target_in(&self.bands.borrow(), self.vertical.get(), x, y)
    }

    /// 登记一次 `WM_MOUSELEAVE`（只登记一次，收到离开后才再登记）。
    fn track_leave(&self) {
        if self.tracking.replace(true) {
            return;
        }
        let mut event = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.inner.hwnd,
            dwHoverTime: 0,
        };
        if unsafe { TrackMouseEvent(&mut event) }.is_err() {
            self.tracking.set(false);
        }
    }
}

/// 客户区 `(x, y)` 落在哪一个可点的东西上。
///
/// `bands.rows` / `bands.sentence` 是内容区坐标，而窗口左上角比内容区往左上多出阴影那一圈，
/// 所以先按 `bands.content` 判一脚（留白与窗口之外都不算命中），再减掉它换进内容坐标系。
/// 竖排候选按 `y` 判（整行都能点，点右侧的词不必对准文字），横排的命中带与高亮底色同一块，`x` 与 `y` 都得在。
fn target_in(bands: &Bands, vertical: bool, x: i32, y: i32) -> Option<Hover> {
    if !inside(bands.content, x, y) {
        return None;
    }
    let (x, y) = (x - bands.content.x as i32, y - bands.content.y as i32);
    // 整句补全画在顶部行右侧、是一块矩形；它在候选行上方，不会跟后者抢。
    if let Some(rect) = bands.sentence
        && inside(rect, x, y)
    {
        return Some(Hover::Trailing);
    }
    bands
        .rows
        .iter()
        .enumerate()
        .find(|(_, rect)| {
            let in_y = y >= rect.y as i32 && y < (rect.y + rect.height) as i32;
            let in_x = x >= rect.x as i32 && x < (rect.x + rect.width) as i32;
            match vertical {
                // 竖排整行都能点：点右侧的词不必对准文字，只判 y。
                true => in_y,
                // 横排只判 x 的话，候选那一列的整条内容区高度都算命中（候选下方点不着的地方也能选词）。
                false => in_x && in_y,
            }
        })
        .map(|(row, _)| Hover::Row(row))
}

/// 点是否落在 `rect`（都相对内容区）里。
fn inside(rect: Rect, x: i32, y: i32) -> bool {
    x >= rect.x as i32
        && x < (rect.x + rect.width) as i32
        && y >= rect.y as i32
        && y < (rect.y + rect.height) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 窗口四周为阴影留出的那一圈（逻辑像素），内容区左上角因此不在客户区原点。
    const MARGIN: i32 = 12;

    /// 两行候选，外加顶部右侧一块整句补全。
    fn bands() -> Bands {
        Bands {
            rows: vec![
                // 20..40，与下一行之间留出 4 的空隙
                Rect {
                    x: 0.0,
                    y: 20.0,
                    width: 200.0,
                    height: 20.0,
                },
                Rect {
                    x: 0.0,
                    y: 44.0,
                    width: 200.0,
                    height: 24.0,
                },
            ],
            sentence: Some(Rect {
                x: 120.0,
                y: 0.0,
                width: 80.0,
                height: 20.0,
            }),
            content: Rect {
                x: MARGIN as f32,
                y: MARGIN as f32,
                width: 200.0,
                height: 68.0,
            },
        }
    }

    /// 客户区坐标 = 阴影留白 + 内容坐标。不减留白的话整条命中范围偏右下，
    /// 竖排会高亮到鼠标下面那一行，整句那块只有一行高、直接点不中。
    #[test]
    fn hit_test_offsets_by_the_shadow_margin() {
        let bands = bands();
        // 压在第一行上
        let (x, y) = (MARGIN + 5, MARGIN + 30);
        assert_eq!(target_in(&bands, true, x, y), Some(Hover::Row(0)));
        assert_eq!(target_in(&bands, false, x, y), Some(Hover::Row(0)));
        // 压在第二行下沿附近
        assert_eq!(
            target_in(&bands, true, MARGIN + 5, MARGIN + 66),
            Some(Hover::Row(1))
        );
        // 压在整句补全那块（内容坐标 y 0..20，在候选行上方）
        assert_eq!(
            target_in(&bands, true, MARGIN + 150, MARGIN + 8),
            Some(Hover::Trailing)
        );
    }

    /// 阴影留白那圈不是可点范围：鼠标挪到窗口边上，高亮要跟着收掉。
    #[test]
    fn the_shadow_margin_is_not_pointable() {
        let bands = bands();
        assert_eq!(target_in(&bands, true, 5, MARGIN + 30), None, "左边留白");
        assert_eq!(target_in(&bands, true, 300, MARGIN + 30), None, "右边留白");
        assert_eq!(target_in(&bands, true, MARGIN + 5, 5), None, "顶部留白");
        assert_eq!(target_in(&bands, true, MARGIN + 5, 200), None, "内容区下方");
        // 内容区里的行间空隙：两行之间也算没压上
        assert_eq!(target_in(&bands, true, MARGIN + 5, MARGIN + 42), None);
    }

    /// 横排的命中带与高亮底色是同一块：候选下方那一片不该还能选中它
    /// （只判 x 的话整条内容区高度都算命中，译文行与底下的空白都点得着）。
    #[test]
    fn horizontal_hits_stop_at_the_band_bottom() {
        let bands = Bands {
            rows: vec![Rect {
                x: 0.0,
                y: 20.0,
                width: 200.0,
                height: 20.0,
            }],
            sentence: None,
            content: Rect {
                x: MARGIN as f32,
                y: MARGIN as f32,
                width: 200.0,
                height: 80.0,
            },
        };
        // 压在候选块上
        assert_eq!(
            target_in(&bands, false, MARGIN + 5, MARGIN + 30),
            Some(Hover::Row(0))
        );
        // 命中带下沿之外（内容坐标 y ≥ 40）不再是它，尽管 x 还在这一列的范围内
        assert_eq!(
            target_in(&bands, false, MARGIN + 5, MARGIN + 50),
            None,
            "横排候选下方不该命中"
        );
        assert_eq!(target_in(&bands, false, MARGIN + 5, MARGIN + 75), None);
        // 竖排照旧按 y 判：压在候选行上照样命中
        assert_eq!(
            target_in(&bands, true, MARGIN + 5, MARGIN + 30),
            Some(Hover::Row(0))
        );
    }
}
