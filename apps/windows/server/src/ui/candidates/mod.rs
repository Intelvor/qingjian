//! 候选窗口：不抢焦点、置顶的分层窗口，跟随光标，画拼音行与候选列表，四周柔和阴影；
//! 鼠标停在候选行或整句补全上铺一层淡底色，点一下选中它——要上屏的文本由 Router 攒着等 DLL 轮询来取。
//!
//! 缺省交给青简渲染器出位图再贴（[`super::painter`]），配置 `renderer = "system"` 时走 GDI：绘制在 [`view`]，
//! 配色 / 字体在 [`theme`]。绘制内容在 [`RenderData`]，一行的展示形态在 [`row`]，鼠标命中在 [`hits`]。
//! GDI 那条退路拿不到行位置，所以只有渲染器模式下有鼠标交互。设计语言对齐 macOS 端。

mod hits;
mod render_data;
pub(crate) mod row;
pub(crate) mod theme;
pub(crate) mod view;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use qingjian_render::{Accent, Hover, Rendered};
use windows::Win32::Foundation::{E_INVALIDARG, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HTCLIENT, IDC_ARROW, LoadCursorW,
    MA_NOACTIVATE, SW_HIDE, SW_SHOWNA, ShowWindow, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_NCHITTEST, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};
use windows::core::{Error, PCWSTR, Result, w};

use qingjian_platform::ThemeMode;
use qingjian_platform::protocol::Frame;

use self::hits::{Bands, Hits};
pub(crate) use self::render_data::RenderData;
use self::theme::Theme;
use super::layered::{self, Layered};
use super::monitor;
use super::painter::SharedPainter;
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

/// 上次贴的窗口位置：悬停换行时按原样再贴一次（悬停不改变窗口位置与大小）。
#[derive(Clone, Copy)]
struct Placement {
    win_pos: (i32, i32),
}

/// 候选窗口的句柄与绘制状态；与命中表共享，窗口过程查到的、要重画的是同一份。
pub(super) struct Inner {
    pub(super) hwnd: HWND,

    /// 绘制内容。
    data: RefCell<RenderData>,

    /// 上次用的 DPI，变了重建字体。
    dpi: Cell<u32>,

    /// 上次解析出的深浅，变了重建配色。
    dark: Cell<bool>,

    /// 主题色（`[general] accent`）。
    accent: Cell<Accent>,

    /// 造当前那份主题时用的主题色，变了重建配色。
    theme_accent: Cell<Accent>,

    /// 青简渲染器；`None` 走 GDI。
    painter: SharedPainter,

    /// 上次贴的几何；还没显示过是 `None`。
    last: Cell<Option<Placement>>,
}

/// 渲染器给出的一帧：位图加这帧里能点的地方。
struct Painted {
    rendered: Rendered,
    bands: Bands,
}

impl Inner {
    /// 刷新内容（不定位、不显示）。
    fn set_content(&self, frame: &Frame) {
        self.data.borrow_mut().set(frame);
    }

    /// 按光标矩形定位并显示：贴光标下方（放不下放上方），四周留出阴影。
    /// 返回这一帧的可点范围；`None` 表示走的是 GDI 退路（拿不到行位置，不做鼠标交互）。
    fn show(&self, anchor: RECT) -> Option<Bands> {
        self.sync_theme();
        let Some(painted) = self.paint(None) else {
            if self.show_gdi(anchor).is_ok() {
                let _ = unsafe { ShowWindow(self.hwnd, SW_SHOWNA) };
            } else {
                self.hide();
            }
            return None;
        };
        let rendered = painted.rendered;
        let content = (
            rendered.content_width as i32,
            rendered.content_height as i32,
        );
        if content.0 <= 0 || content.1 <= 0 {
            self.hide();
            return Some(painted.bands);
        }
        let (content_x, content_y) = place(anchor, content);
        let win_pos = (
            content_x - rendered.content_x as i32,
            content_y - rendered.content_y as i32,
        );
        if self.present(&rendered, win_pos).is_err() {
            self.hide();
            return Some(painted.bands);
        }
        let _ = unsafe { ShowWindow(self.hwnd, SW_SHOWNA) };
        self.last.set(Some(Placement { win_pos }));
        Some(painted.bands)
    }

    /// 只换悬停：位置与尺寸照旧，重贴一次。还没显示过、或走 GDI 就什么也不做。
    fn repaint(&self, hover: Option<Hover>) {
        let Some(placement) = self.last.get() else {
            return;
        };
        match self.paint(hover) {
            Some(painted) => {
                if let Err(error) = self.present(&painted.rendered, placement.win_pos) {
                    tracing::warn!(%error, "候选窗口重画失败");
                }
            }
            // GDI 退路画不了悬停，也不能让窗口空掉：按原样再合成一次。
            None => {
                let _ = self.show_gdi_at(placement);
            }
        }
    }

    /// 用渲染器画一帧（带这帧能点的地方）；没有渲染器返回 `None`。
    fn paint(&self, hover: Option<Hover>) -> Option<Painted> {
        let data = self.data.borrow();
        let rendered = self.painter.borrow_mut().as_mut().and_then(|painter| {
            painter.render_frame(
                &data.render_frame(hover),
                data.layout,
                self.dark.get(),
                self.dpi.get(),
            )
        })?;
        Some(Painted {
            bands: Bands {
                rows: rendered.rows.clone(),
                sentence: rendered.sentence,
                content: qingjian_render::Rect {
                    x: rendered.content_x as f32,
                    y: rendered.content_y as f32,
                    width: rendered.content_width as f32,
                    height: rendered.content_height as f32,
                },
            },
            rendered,
        })
    }

    fn present(&self, rendered: &Rendered, win_pos: (i32, i32)) -> Result<()> {
        layered::present(self.hwnd, &rendered.pixmap, win_pos)
    }

    /// GDI 画法：量尺寸、定位、合成。
    fn show_gdi(&self, anchor: RECT) -> Result<()> {
        let content = self.preferred_size();
        if content.0 <= 0 || content.1 <= 0 {
            return Err(Error::from(E_INVALIDARG));
        }
        let (content_x, content_y) = place(anchor, content);
        self.compose_gdi(content, content_x, content_y)
    }

    /// 悬停重画落到 GDI 时：按上次的位置合成一次。
    fn show_gdi_at(&self, placement: Placement) -> Result<()> {
        let margin = layered::shadow_margin(self.dpi.get());
        let content = self.preferred_size();
        self.compose_gdi(
            content,
            placement.win_pos.0 + margin,
            placement.win_pos.1 + margin,
        )
    }

    fn compose_gdi(&self, content: (i32, i32), content_x: i32, content_y: i32) -> Result<()> {
        let margin = layered::shadow_margin(self.dpi.get());
        let data = self.data.borrow();
        layered::composite(
            self.hwnd,
            &Layered {
                content,
                margin,
                win_pos: (content_x - margin, content_y - margin),
                win_size: (content.0 + margin * 2, content.1 + margin * 2),
                background: data.theme.background,
                corner_radius: data.theme.corner_radius,
                paint: &|hdc, client| view::paint(hdc, &data, client),
            },
        )
    }

    fn hide(&self) {
        let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
    }

    /// 当前排布（竖排按 y 判命中、横排按 x）。
    fn vertical(&self) -> bool {
        self.data.borrow().vertical()
    }

    /// DPI、深浅或主题色变了就重建主题；每次 `show` 前调。
    fn sync_theme(&self) {
        let dpi = match unsafe { GetDpiForWindow(self.hwnd) } {
            0 => self.dpi.get(),
            dpi => dpi,
        };
        let dark = resolve_dark(self.data.borrow().theme_mode);
        let accent = self.accent.get();
        if dpi != self.dpi.get() || dark != self.dark.get() || accent != self.theme_accent.get() {
            self.data.borrow_mut().theme = Rc::new(Theme::new(dpi, dark, accent));
            self.dpi.set(dpi);
            self.dark.set(dark);
            self.theme_accent.set(accent);
        }
    }

    /// 内容需要的大小（不含阴影留白）。
    fn preferred_size(&self) -> (i32, i32) {
        let hdc = unsafe { GetDC(Some(self.hwnd)) };
        let size = view::preferred_size(hdc, &self.data.borrow());
        unsafe { ReleaseDC(Some(self.hwnd), hdc) };
        (size.cx, size.cy)
    }
}

/// 候选窗口。内容经 `UpdateLayeredWindow` 一次贴上；鼠标点在候选行或整句补全上可选词。
pub(crate) struct CandidateWindow {
    /// 窗口句柄与绘制状态。
    inner: Rc<Inner>,

    /// 鼠标命中状态（窗口过程按 HWND 查到同一份）。
    hits: Rc<Hits>,
}

impl CandidateWindow {
    /// 建一个隐藏的候选窗口；点击候选经 `events` 回给 Router。
    pub(crate) fn new(events: CandidateEvents, painter: SharedPainter) -> Result<Self> {
        CLASS.ensure(|| WNDCLASSEXW {
            lpfnWndProc: Some(wndproc),
            hInstance: super::module_handle(),
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        })?;
        let dpi = unsafe { GetDpiForSystem() }.max(96);
        let dark = resolve_dark(ThemeMode::default());
        let data = RefCell::new(RenderData::empty(Rc::new(Theme::new(
            dpi,
            dark,
            Accent::default(),
        ))));
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
            accent: Cell::new(Accent::default()),
            theme_accent: Cell::new(Accent::default()),
            painter,
            last: Cell::new(None),
        });
        let hits = Hits::attach(inner.clone(), events);
        Ok(Self { inner, hits })
    }

    /// 刷新内容（不定位、不显示）。
    pub(crate) fn set_content(&self, frame: &Frame) {
        self.inner.set_content(frame);
    }

    /// 换主题色（`[general] accent`）：下一次显示时重建配色。
    pub(crate) fn set_accent(&self, accent: Accent) {
        self.inner.accent.set(accent);
    }

    /// 按光标矩形定位并显示，顺带把这帧的可点范围交给命中表。
    pub(crate) fn show(&self, anchor: RECT) {
        let vertical = self.inner.vertical();
        match self.inner.show(anchor) {
            Some(bands) => self.hits.set_bands(bands, vertical),
            // GDI 退路给不出行位置：清掉命中范围，别拿上一帧的矩形让人乱点。
            None => self.hits.set_bands(Bands::default(), vertical),
        }
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

/// 分层窗口无需 `WM_PAINT`；鼠标消息用来在候选行与整句补全上悬停 / 点选，其余交默认处理。
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
