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

/// 一帧里能点的地方：候选行的矩形与整句补全的矩形（都相对内容区左上角）。
#[derive(Default)]
pub(super) struct Bands {
    pub(super) rows: Vec<Rect>,
    pub(super) sentence: Option<Rect>,
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
        let bands = self.bands.borrow();
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
            .find(|(_, rect)| match self.vertical.get() {
                true => y >= rect.y as i32 && y < (rect.y + rect.height) as i32,
                false => x >= rect.x as i32 && x < (rect.x + rect.width) as i32,
            })
            .map(|(row, _)| Hover::Row(row))
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

/// 点是否落在 `rect`（都相对内容区）里。
fn inside(rect: Rect, x: i32, y: i32) -> bool {
    x >= rect.x as i32
        && x < (rect.x + rect.width) as i32
        && y >= rect.y as i32
        && y < (rect.y + rect.height) as i32
}
