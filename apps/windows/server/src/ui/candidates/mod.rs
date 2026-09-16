//! 候选窗口：不抢焦点、置顶的分层窗口，跟随光标，画拼音行与候选列表，四周柔和阴影；
//! 鼠标停在候选行上会浅高亮，点一下选中它上屏。
//! 绘制在 [`view`]，绘制内容在 [`RenderData`]，一行的展示形态在 [`row`]，配色 / 字体在 [`theme`]，
//! 鼠标命中在 [`hits`]。设计语言对齐 macOS 端。

mod hits;
mod render_data;
pub(crate) mod row;
pub(crate) mod theme;
pub(crate) mod view;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HTCLIENT, IDC_ARROW, LoadCursorW,
    MA_NOACTIVATE, SW_HIDE, SW_SHOWNA, ShowWindow, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_NCHITTEST, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};
use windows::core::{PCWSTR, Result, w};

use qingjian_platform::LayoutMode;
use qingjian_platform::ThemeMode;
use qingjian_platform::protocol::Frame;

use self::hits::{Hits, Target};
pub(crate) use self::render_data::RenderData;
use self::theme::Theme;
use self::view::Bands;
use super::layered::{self, Layered};
use super::monitor;
use super::window_class::WindowClass;
use crate::ui::CandidateEvents;

const CLASS_NAME: PCWSTR = w!("QingjianCandidateWindow");
static CLASS: WindowClass = WindowClass::new();

/// 光标行与候选窗之间的间隙（逻辑像素）。
const CARET_GAP: i32 = 2;

/// 按外观模式解析深浅；`System` 读系统主题。
pub(super) fn resolve_dark(mode: ThemeMode) -> bool {
    match mode {
        ThemeMode::Light => false,
        ThemeMode::Dark => true,
        ThemeMode::System => system_prefers_dark(),
    }
}

/// `HKCU\...\Themes\Personalize\AppsUseLightTheme` 为 0 是深色；读不到当浅色。
fn system_prefers_dark() -> bool {
    windows_registry::CURRENT_USER
        .open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_u32("AppsUseLightTheme"))
        .is_ok_and(|value| value == 0)
}

/// 一次合成用的几何；悬停重画照原样再贴一次（悬停不改变尺寸与位置）。
#[derive(Clone, Copy)]
struct Layout {
    /// 内容区大小（不含阴影留白）。
    content: (i32, i32),

    /// 四周的阴影留白。
    margin: i32,

    /// 窗口左上角（含留白）。
    win_pos: (i32, i32),
}

/// 候选窗口的句柄与绘制状态；与命中表共享（悬停换行要重贴一次）。
struct Inner {
    hwnd: HWND,

    /// 绘制内容。
    data: RefCell<RenderData>,

    /// 上次用的 DPI，变了重建字体。
    dpi: Cell<u32>,

    /// 上次解析出的深浅，变了重建配色。
    dark: Cell<bool>,

    /// 上次合成用的几何；还没显示过时是 `None`。
    last: Cell<Option<Layout>>,
}

impl Inner {
    /// 刷新内容（不定位、不显示）。
    fn set_content(&self, frame: &Frame) {
        self.data.borrow_mut().set(frame);
    }

    /// 按光标矩形定位并显示：贴光标下方（放不下放上方），四周留出阴影。
    fn show(&self, anchor: RECT) {
        self.sync_theme();
        let content = self.preferred_size();
        if content.0 <= 0 || content.1 <= 0 {
            self.hide();
            return;
        }
        let margin = self.margin();
        let (content_x, content_y) = place(anchor, content);
        let layout = Layout {
            content,
            margin,
            win_pos: (content_x - margin, content_y - margin),
        };
        if self.compose(layout, None).is_ok() {
            let _ = unsafe { ShowWindow(self.hwnd, SW_SHOWNA) };
        } else {
            self.hide();
        }
    }

    /// 只换悬停、位置与尺寸照旧：照上次的几何重贴一次。还没显示过就什么都不做。
    fn repaint(&self, hover: Option<Target>) {
        if let Some(layout) = self.last.get()
            && let Err(error) = self.compose(layout, hover)
        {
            tracing::warn!(%error, "候选窗口重画失败");
        }
    }

    fn hide(&self) {
        let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
    }

    /// 内容需要的大小（不含阴影留白）。
    fn preferred_size(&self) -> (i32, i32) {
        let hdc = unsafe { GetDC(Some(self.hwnd)) };
        let size = view::preferred_size(hdc, &self.data.borrow());
        unsafe { ReleaseDC(Some(self.hwnd), hdc) };
        (size.cx, size.cy)
    }

    /// 这一帧里能点的地方（候选行带 + 整句补全），供鼠标定位。
    fn bands(&self) -> Bands {
        let hdc = unsafe { GetDC(Some(self.hwnd)) };
        let bands = view::hit_bands(hdc, &self.data.borrow());
        unsafe { ReleaseDC(Some(self.hwnd), hdc) };
        bands
    }

    /// 当前排布（竖排按 y 判命中、横排按 x）。
    fn layout_mode(&self) -> LayoutMode {
        self.data.borrow().layout
    }

    /// 四周的阴影留白（按当前 DPI）。
    fn margin(&self) -> i32 {
        layered::shadow_margin(self.dpi.get())
    }

    /// DPI 或深浅变了就重建主题；每次 `show` 前调。
    fn sync_theme(&self) {
        let dpi = match unsafe { GetDpiForWindow(self.hwnd) } {
            0 => self.dpi.get(),
            dpi => dpi,
        };
        let dark = resolve_dark(self.data.borrow().theme_mode);
        if dpi != self.dpi.get() || dark != self.dark.get() {
            self.data.borrow_mut().theme = Rc::new(Theme::new(dpi, dark));
            self.dpi.set(dpi);
            self.dark.set(dark);
        }
    }

    /// 贴上按 `layout` 合成好的一帧，`hover` 是鼠标停住的地方。
    fn compose(&self, layout: Layout, hover: Option<Target>) -> Result<()> {
        let updated = {
            let data = self.data.borrow();
            layered::composite(
                self.hwnd,
                &Layered {
                    content: layout.content,
                    margin: layout.margin,
                    win_pos: layout.win_pos,
                    win_size: (
                        layout.content.0 + layout.margin * 2,
                        layout.content.1 + layout.margin * 2,
                    ),
                    background: data.theme.background,
                    corner_radius: data.theme.corner_radius,
                    paint: &|hdc, client| view::paint(hdc, &data, client, hover),
                },
            )
        };
        if updated.is_ok() {
            self.last.set(Some(layout));
        }
        updated
    }
}

/// 候选窗口。内容经 `UpdateLayeredWindow` 一次贴上；鼠标点在候选行上选词。
pub(crate) struct CandidateWindow {
    /// 窗口句柄与绘制状态。
    inner: Rc<Inner>,

    /// 鼠标命中状态（窗口过程按 HWND 查到同一个）。
    hits: Rc<Hits>,
}

impl CandidateWindow {
    /// 建一个隐藏的候选窗口；点击候选经 `events` 回给 Router。
    pub(crate) fn new(events: CandidateEvents) -> Result<Self> {
        CLASS.ensure(|| WNDCLASSEXW {
            lpfnWndProc: Some(wndproc),
            hInstance: super::module_handle(),
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        })?;
        let dpi = unsafe { GetDpiForSystem() }.max(96);
        let dark = resolve_dark(ThemeMode::default());
        let data = RefCell::new(RenderData::empty(Rc::new(Theme::new(dpi, dark))));
        // NOACTIVATE：显示时不抢应用焦点。
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                CLASS_NAME,
                w!("青简候选"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(super::module_handle()),
                None,
            )?
        };
        let inner = Rc::new(Inner {
            hwnd,
            data,
            dpi: Cell::new(dpi),
            dark: Cell::new(dark),
            last: Cell::new(None),
        });
        let hits = Hits::attach(inner.clone(), layered::shadow_margin(dpi), events);
        Ok(Self { inner, hits })
    }

    /// 刷新内容（不定位、不显示）。
    pub(crate) fn set_content(&self, frame: &Frame) {
        self.inner.set_content(frame);
    }

    /// 按光标矩形定位并显示，顺带把这一帧的行位置换成命中带。
    pub(crate) fn show(&self, anchor: RECT) {
        self.inner.show(anchor);
        let vertical = self.inner.layout_mode() == LayoutMode::Vertical;
        let bands = self.inner.bands();
        self.hits.set_bands(self.inner.margin(), vertical, bands);
    }

    pub(crate) fn hide(&self) {
        self.inner.hide();
        self.hits.on_hide();
    }
}

impl Drop for CandidateWindow {
    fn drop(&mut self) {
        Hits::detach(self.inner.hwnd);
        let _ = unsafe { DestroyWindow(self.inner.hwnd) };
    }
}

/// 内容左上角：贴光标下方，放不下放上方，再放不下贴屏幕内；都夹在所在显示器工作区里。
fn place(anchor: RECT, content: (i32, i32)) -> (i32, i32) {
    let work = monitor::work_area_near(POINT {
        x: anchor.left,
        y: anchor.top,
    });
    let x = anchor
        .left
        .clamp(work.left, (work.right - content.0).max(work.left));
    let below = anchor.bottom + CARET_GAP;
    let above = anchor.top - CARET_GAP - content.1;
    let y = if below + content.1 <= work.bottom {
        below
    } else if above >= work.top {
        above
    } else {
        (work.bottom - content.1).max(work.top)
    };
    (x, y)
}

/// 分层窗口无需 `WM_PAINT`；鼠标消息用来在候选行上悬停 / 点选，其余交默认处理。
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // 点候选窗不抢宿主焦点：宿主还要接着收键，组句不能断。
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_NCHITTEST => LRESULT(HTCLIENT as isize),
        WM_MOUSEMOVE => {
            if let Some(hits) = Hits::of(hwnd) {
                let (x, y) = client_pos(lparam);
                hits.on_mouse_move(x, y);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            if let Some(hits) = Hits::of(hwnd) {
                hits.on_mouse_leave();
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if let Some(hits) = Hits::of(hwnd) {
                let (x, y) = client_pos(lparam);
                hits.on_click(x, y);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// 鼠标消息 lparam 低 16 位是客户区 x、高 16 位是 y（都有符号）。
fn client_pos(lparam: LPARAM) -> (i32, i32) {
    (
        (lparam.0 & 0xFFFF) as i16 as i32,
        ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
    )
}
