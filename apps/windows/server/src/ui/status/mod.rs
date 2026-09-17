//! 悬浮状态条：可拖动的四格浮窗 `[中 / 英][，。/ ,.][☁][⚙]`，显示前台应用当前的中英模式。缺省由青简渲染器画
//! （[`super::painter`]），`renderer = "system"` 时走 GDI：复用候选窗口的主题与分层窗口合成器。
//!
//! 按下鼠标先 `DragDetect`：挪出拖动阈值就交给系统的移动循环（`WM_NCLBUTTONDOWN` + `HTCAPTION`），
//! 结束时 `WM_EXITSIZEMOVE` 报新位置；没挪就是点击，按落在哪格分发做事。`WM_MOUSEACTIVATE` 回
//! `MA_NOACTIVATE` 不抢焦点。鼠标停在某格上时那一格铺一层比键盘高亮淡一档的底色——
//! 可点的范围与那层底色同一块（渲染器给的，见 `RenderedStatus::cell_rects`），所以看得见高亮就是点得着的。
//!
//! 摆放、绘制与命中是一份状态（`Bar`）：窗口过程要能重画，就得跟绘制用的是同一份数据。
//! 一格的规格在 [`cell`]。

mod cell;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::mem::size_of;
use std::rc::Rc;

use qingjian_render::{Accent, Rect, StatusCell};
use windows::Win32::Foundation::{
    CloseHandle, E_INVALIDARG, FALSE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, TRUE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    GetDC, HDC, ReleaseDC, ScreenToClient, SetBkMode, TRANSPARENT,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    DragDetect, ReleaseCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, EnumWindows, GW_OWNER, GetCursorPos, GetWindow,
    GetWindowRect, GetWindowThreadProcessId, HTCAPTION, HTCLIENT, IDC_HAND, IsWindowVisible,
    LoadCursorW, MA_NOACTIVATE, SW_HIDE, SW_RESTORE, SW_SHOWNA, SendMessageW, SetForegroundWindow,
    ShowWindow, WM_EXITSIZEMOVE, WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NCHITTEST,
    WM_NCLBUTTONDOWN, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Error, PCWSTR, PWSTR, Result, w};

use qingjian_platform::ThemeMode;

use self::cell::CellSpec;
use super::StatusEvents;
use super::candidates::resolve_dark;
use super::candidates::theme::Theme;
use super::candidates::view;
use super::layered::{self, Layered};
use super::monitor;
use super::painter::SharedPainter;
use super::window_class::WindowClass;
use crate::dispatch::{StatusEvent, StatusView};

const CLASS_NAME: PCWSTR = w!("QingjianStatusBar");
static CLASS: WindowClass = WindowClass::new();

/// 状态条与屏幕边缘的间隙（逻辑像素）。
const EDGE_GAP: i32 = 8;

/// 状态条上一格点下去做什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusAction {
    /// 「中 / 英」：切模式。
    ToggleMode,

    /// 「，。」：切全角标点（中英各记一份）。
    TogglePunctuation,

    /// 「☁」：开 / 关在线联想（隐私开关，写回 `[predict] enabled` 并当场换 Predictor）。
    ToggleCloud,

    /// 齿轮：打开设置程序（UI 线程直接起进程，不经 Router）。
    OpenSettings,
}

/// 四格从左到右的动作，与 [`Bar::status_cells`] / [`Bar::cells`] 的顺序一一对应。
const ACTIONS: [StatusAction; 4] = [
    StatusAction::ToggleMode,
    StatusAction::TogglePunctuation,
    StatusAction::ToggleCloud,
    StatusAction::OpenSettings,
];

thread_local! {
    /// 本线程活着的状态条：HWND → 状态。窗口过程按 HWND 查，查不到（已析构）就忽略。
    static BARS: RefCell<HashMap<isize, Rc<Bar>>> = RefCell::new(HashMap::new());
}

/// 绘制、摆放与鼠标命中共用的一份状态。
struct Bar {
    /// 状态条窗口。
    hwnd: HWND,

    /// 最近一次要显示的内容；还没显示过时为 `None`。
    data: RefCell<Option<StatusView>>,

    /// 按 DPI / 深浅造好的主题（复用候选窗口那套）。
    theme: RefCell<Rc<Theme>>,

    /// 上次用的 DPI，变了重建主题。
    dpi: Cell<u32>,

    /// 上次解析出的深浅，变了重建配色。
    dark: Cell<bool>,

    /// 主题色（`[general] accent`）。
    accent: Cell<Accent>,

    /// 造当前那份主题时用的主题色，变了重建配色。
    theme_accent: Cell<Accent>,

    /// 青简渲染器；`None` 走 GDI。
    painter: SharedPainter,

    /// 上次合成用的阴影留白：拖动结束从窗口矩形反推内容左上角，点 / 悬停时把客户区换成内容坐标。
    margin: Cell<i32>,

    /// 内容左上角的屏幕坐标（物理像素）；`None` 表示还没摆放过。
    pos: Cell<Option<(i32, i32)>>,

    /// 上次画出的各格命中矩形（内容坐标）与动作，从左到右。
    bands: RefCell<Vec<(Rect, StatusAction)>>,

    /// 鼠标悬停的格号；不在任何格上为 `None`。
    hover: Cell<Option<usize>>,

    /// 已登记 `WM_MOUSELEAVE`；鼠标每次移进窗口登记一次，收到离开后要重登记。
    tracking: Cell<bool>,

    /// 点格 / 拖动结束回给 Router。
    events: StatusEvents,
}

impl Bar {
    /// 建好并登记到本线程的 HWND 表里。
    fn attach(
        hwnd: HWND,
        dpi: u32,
        dark: bool,
        painter: SharedPainter,
        events: StatusEvents,
    ) -> Rc<Self> {
        let bar = Rc::new(Self {
            hwnd,
            data: RefCell::new(None),
            theme: RefCell::new(Rc::new(Theme::new(dpi, dark, Accent::default()))),
            dpi: Cell::new(dpi),
            dark: Cell::new(dark),
            accent: Cell::new(Accent::default()),
            theme_accent: Cell::new(Accent::default()),
            painter,
            margin: Cell::new(layered::shadow_margin(dpi)),
            pos: Cell::new(None),
            bands: RefCell::new(Vec::new()),
            hover: Cell::new(None),
            tracking: Cell::new(false),
            events,
        });
        BARS.with(|map| map.borrow_mut().insert(hwnd.0 as isize, bar.clone()));
        bar
    }

    fn detach(hwnd: HWND) {
        BARS.with(|map| map.borrow_mut().remove(&(hwnd.0 as isize)));
    }

    /// 窗口过程按 HWND 查；查不到返回 `None`。
    fn of(hwnd: HWND) -> Option<Rc<Self>> {
        BARS.with(|map| map.borrow().get(&(hwnd.0 as isize)).cloned())
    }

    /// 显示 / 更新：按记住的位置（首次用 `view.anchor`，都没有就右下角）摆放并重绘。
    fn update(&self, view: StatusView) {
        if self.pos.get().is_none() {
            self.pos.set(view.anchor);
        }
        *self.data.borrow_mut() = Some(view);
        self.sync_theme();
        self.render();
    }

    fn hide(&self) {
        self.hover.set(None);
        self.tracking.set(false);
        self.bands.borrow_mut().clear();
        let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
    }

    /// DPI、深浅或主题色变了就重建主题。
    fn sync_theme(&self) {
        let dpi = match unsafe { GetDpiForWindow(self.hwnd) } {
            0 => self.dpi.get(),
            dpi => dpi,
        };
        let mode = self
            .data
            .borrow()
            .as_ref()
            .map(|view| view.theme)
            .unwrap_or_default();
        let dark = resolve_dark(mode);
        let accent = self.accent.get();
        if dpi != self.dpi.get() || dark != self.dark.get() || accent != self.theme_accent.get() {
            *self.theme.borrow_mut() = Rc::new(Theme::new(dpi, dark, accent));
            self.dpi.set(dpi);
            self.dark.set(dark);
            self.theme_accent.set(accent);
        }
    }

    /// 模式格的文字：中 / 英 / 注，开着双拼时跟方案名。
    fn mode_text(view: &StatusView) -> String {
        if view.english {
            "英".to_owned()
        } else if view.zhuyin {
            "注".to_owned()
        } else {
            match &view.scheme {
                Some(scheme) => format!("中 · {scheme}"),
                None => "中".to_owned(),
            }
        }
    }

    /// 渲染器要的四格，顺序同 [`ACTIONS`]：模式（品牌色）、标点（生效时品牌色）、☁（开着品牌色）、齿轮。
    fn status_cells(view: &StatusView) -> Vec<StatusCell> {
        [
            StatusCell::text(Self::mode_text(view), true),
            StatusCell::text(if view.full_width { "，。" } else { ",." }, view.full_width),
            StatusCell::cloud(view.cloud),
            StatusCell::Gear,
        ]
        .into_iter()
        .collect()
    }

    /// GDI 画法的四格，顺序同 [`ACTIONS`]。
    fn cells(&self, theme: &Theme) -> Vec<CellSpec> {
        let data = self.data.borrow();
        let Some(view) = data.as_ref() else {
            return Vec::new();
        };
        [
            CellSpec {
                text: Self::mode_text(view),
                font: theme.text_font,
                color: theme.accent_color,
                action: StatusAction::ToggleMode,
            },
            CellSpec {
                text: if view.full_width { "，。" } else { ",." }.to_owned(),
                font: theme.text_font,
                color: if view.full_width {
                    theme.accent_color
                } else {
                    theme.gloss_color
                },
                action: StatusAction::TogglePunctuation,
            },
            CellSpec {
                text: "\u{2601}".to_owned(),
                font: theme.symbol_font,
                color: if view.cloud {
                    theme.accent_color
                } else {
                    theme.gloss_color
                },
                action: StatusAction::ToggleCloud,
            },
            CellSpec {
                text: "\u{2699}".to_owned(),
                font: theme.symbol_font,
                color: theme.gloss_color,
                action: StatusAction::OpenSettings,
            },
        ]
        .into_iter()
        .collect()
    }

    /// 画好贴上并显示；顺带记下各格的命中矩形。渲染器画不成就走 GDI。
    fn render(&self) {
        let hovered = self.hover.get();
        let rendered = {
            let data = self.data.borrow();
            let mut painter = self.painter.borrow_mut();
            match (data.as_ref(), painter.as_mut()) {
                (Some(view), Some(painter)) => painter.render_status(
                    &Self::status_cells(view),
                    self.dark.get(),
                    self.dpi.get(),
                    hovered,
                ),
                _ => None,
            }
        };
        let updated = match rendered {
            Some(rendered) => {
                let bitmap = &rendered.rendered;
                let content = (bitmap.content_width as i32, bitmap.content_height as i32);
                if content.0 <= 0 || content.1 <= 0 {
                    self.hide();
                    return;
                }
                let margin = bitmap.content_x as i32;
                self.margin.set(margin);
                *self.bands.borrow_mut() = rendered
                    .cell_rects
                    .iter()
                    .zip(ACTIONS)
                    .map(|(rect, action)| (*rect, action))
                    .collect();
                let anchor = self.anchor(content, margin);
                layered::present(
                    self.hwnd,
                    &bitmap.pixmap,
                    (anchor.0 - margin, anchor.1 - margin),
                )
            }
            None => self.render_gdi(hovered),
        };
        if updated.is_ok() {
            let _ = unsafe { ShowWindow(self.hwnd, SW_SHOWNA) };
        } else {
            self.hide();
        }
        // 窗口挪了位置时，鼠标底下那一格可能变了（拖动结束后鼠标往往没再动）。
        self.refresh_hover();
    }

    /// 内容左上角：记住的位置，没有就右下角，再夹进工作区；顺带记下。
    fn anchor(&self, content: (i32, i32), margin: i32) -> (i32, i32) {
        let anchor = self
            .pos
            .get()
            .unwrap_or_else(|| default_anchor(content, margin));
        let anchor = clamp_anchor(anchor, content, margin);
        self.pos.set(Some(anchor));
        anchor
    }

    /// GDI 画法：量各格、算内容尺寸、摆位置、合成贴上。
    fn render_gdi(&self, hovered: Option<usize>) -> Result<()> {
        let theme = self.theme.borrow().clone();
        let margin = layered::shadow_margin(self.dpi.get());
        self.margin.set(margin);
        let cells = self.cells(&theme);
        let hdc = unsafe { GetDC(Some(self.hwnd)) };
        let sizes: Vec<SIZE> = cells
            .iter()
            .map(|cell| view::measure(hdc, cell.font, &cell.text))
            .collect();
        unsafe { ReleaseDC(Some(self.hwnd), hdc) };
        let line = sizes.iter().map(|size| size.cy).max().unwrap_or(0);
        // 每格：文字左右各留一个半 padding（悬停底色内缩半个 padding，正好让开格间那条细线）。
        let widths: Vec<i32> = sizes
            .iter()
            .map(|size| size.cx + theme.padding * 3)
            .collect();
        let content = (widths.iter().sum::<i32>(), line + theme.padding);
        if content.0 <= 0 || content.1 <= 0 || cells.is_empty() {
            return Err(Error::from(E_INVALIDARG));
        }
        let inset = theme.padding / 2;
        let mut left = 0;
        let bands: Vec<(Rect, StatusAction)> = cells
            .iter()
            .zip(&widths)
            .map(|(cell, width)| {
                let band = Rect {
                    x: (left + inset) as f32,
                    y: (inset / 2) as f32,
                    width: (width - inset * 2) as f32,
                    height: (content.1 - inset) as f32,
                };
                left += width;
                (band, cell.action)
            })
            .collect();
        *self.bands.borrow_mut() = bands;
        let anchor = self.anchor(content, margin);

        layered::composite(
            self.hwnd,
            &Layered {
                content,
                margin,
                win_pos: (anchor.0 - margin, anchor.1 - margin),
                win_size: (content.0 + margin * 2, content.1 + margin * 2),
                background: theme.background,
                corner_radius: theme.corner_radius,
                paint: &|hdc, client| {
                    unsafe { SetBkMode(hdc, TRANSPARENT) };
                    paint_cells(hdc, client, &cells, &sizes, &widths, &theme, hovered);
                },
            },
        )
    }

    /// 拖动结束：从窗口矩形反推内容左上角，记下并交给 Router 写回配置。
    fn on_moved(&self) {
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(self.hwnd, &mut rect) }.is_ok() {
            let margin = self.margin.get();
            let x = rect.left + margin;
            let y = rect.top + margin;
            self.pos.set(Some((x, y)));
            (self.events)(StatusEvent::Moved(x, y));
        }
    }

    /// 单击：落进哪格就做哪格的事。
    fn on_click(&self, x: i32, y: i32) {
        let action = self
            .cell_at(x, y)
            .and_then(|index| self.bands.borrow().get(index).map(|(_, action)| *action));
        match action {
            Some(StatusAction::ToggleMode) => (self.events)(StatusEvent::ToggleMode),
            Some(StatusAction::TogglePunctuation) => {
                (self.events)(StatusEvent::TogglePunctuation);
            }
            Some(StatusAction::ToggleCloud) => (self.events)(StatusEvent::ToggleCloud),
            Some(StatusAction::OpenSettings) => open_settings(),
            None => {}
        }
    }

    /// 鼠标移动：换格就重画一次（底色跟着走）。
    fn on_mouse_move(&self, x: i32, y: i32) {
        self.track_leave();
        self.set_hover(self.cell_at(x, y));
    }

    fn on_mouse_leave(&self) {
        self.tracking.set(false);
        self.set_hover(None);
    }

    fn set_hover(&self, hover: Option<usize>) {
        if hover != self.hover.get() {
            self.hover.set(hover);
            self.render();
        }
    }

    /// 按鼠标当前位置重算悬停（窗口自己挪过位置，而鼠标没动时收不到消息）。
    fn refresh_hover(&self) {
        let mut point = POINT::default();
        if unsafe { GetCursorPos(&mut point) }.is_err()
            || !unsafe { ScreenToClient(self.hwnd, &mut point) }.as_bool()
        {
            return;
        }
        self.set_hover(self.cell_at(point.x, point.y));
    }

    /// 客户区坐标落在第几格上。命中矩形是渲染器给的（与底色同一块），边缘那圈自然点不着。
    fn cell_at(&self, x: i32, y: i32) -> Option<usize> {
        let margin = self.margin.get();
        let (x, y) = (x - margin, y - margin);
        self.bands.borrow().iter().position(|(rect, _)| {
            x >= rect.x as i32
                && x < (rect.x + rect.width) as i32
                && y >= rect.y as i32
                && y < (rect.y + rect.height) as i32
        })
    }

    /// 登记一次 `WM_MOUSELEAVE`（只登记一次，收到离开后才再登记）。
    fn track_leave(&self) {
        if self.tracking.replace(true) {
            return;
        }
        let mut event = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.hwnd,
            dwHoverTime: 0,
        };
        if unsafe { TrackMouseEvent(&mut event) }.is_err() {
            self.tracking.set(false);
        }
    }
}

/// 悬浮状态条窗口：只是 [`Bar`] 的壳，绘制与命中的状态在 `Bar` 上（窗口过程查得到）。
pub(super) struct StatusBar {
    bar: Rc<Bar>,
}

impl StatusBar {
    /// 建一个隐藏的状态条窗口。
    pub(super) fn new(events: StatusEvents, painter: SharedPainter) -> Result<Self> {
        CLASS.ensure(|| WNDCLASSEXW {
            lpfnWndProc: Some(wndproc),
            hInstance: super::module_handle(),
            hCursor: unsafe { LoadCursorW(None, IDC_HAND) }.unwrap_or_default(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        })?;
        let dpi = unsafe { GetDpiForSystem() }.max(96);
        let dark = resolve_dark(ThemeMode::default());
        // NOACTIVATE：显示时不抢应用焦点。
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                CLASS_NAME,
                w!("青简状态条"),
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
        Ok(Self {
            bar: Bar::attach(hwnd, dpi, dark, painter, events),
        })
    }

    /// 显示 / 更新。
    pub(super) fn update(&self, view: StatusView) {
        self.bar.update(view);
    }

    pub(super) fn hide(&self) {
        self.bar.hide();
    }

    /// 换主题色（`[general] accent`）：下一次刷新时重建配色。
    pub(super) fn set_accent(&self, accent: Accent) {
        self.bar.accent.set(accent);
    }
}

impl Drop for StatusBar {
    fn drop(&mut self) {
        Bar::detach(self.bar.hwnd);
        let _ = unsafe { DestroyWindow(self.bar.hwnd) };
    }
}

/// 每格文字居中；格间一条上下留 `inset` 的细线；鼠标停住那格先铺一层浅底色（左右各内缩 `inset`，让开细线）。
/// 每格文字居中；格间一条上下留 `inset` 的细线；鼠标停住那格先铺一层浅底色（左右各内缩 `inset`，让开细线）。
/// 颜色与留白都从 `theme` 取，免得参数摊一长串。
fn paint_cells(
    hdc: HDC,
    client: RECT,
    cells: &[CellSpec],
    sizes: &[SIZE],
    widths: &[i32],
    theme: &Theme,
    hovered: Option<usize>,
) {
    let inset = theme.padding / 2;
    let mut x = 0;
    for (index, ((cell, size), width)) in cells.iter().zip(sizes).zip(widths).enumerate() {
        if index > 0 {
            view::fill_rect(
                hdc,
                RECT {
                    left: x,
                    top: inset,
                    right: x + 1,
                    bottom: client.bottom - inset,
                },
                theme.pos_color,
            );
        }
        if hovered == Some(index) {
            view::fill_round_rect(
                hdc,
                RECT {
                    left: x + inset,
                    top: inset / 2,
                    right: x + width - inset,
                    bottom: client.bottom - inset / 2,
                },
                theme.hover_color,
                inset,
            );
        }
        let ox = x + (width - size.cx) / 2;
        let oy = (client.bottom - size.cy) / 2;
        view::draw_text(hdc, cell.font, cell.color, ox, oy, &cell.text);
        x += width;
    }
}

/// 首次出现的位置：主显示器工作区右下角，留出边距与阴影。
fn default_anchor(content: (i32, i32), margin: i32) -> (i32, i32) {
    let work = monitor::primary_work_area();
    let gap = ((EDGE_GAP * margin) / 16).max(EDGE_GAP);
    (
        work.right - margin - gap - content.0,
        work.bottom - margin - gap - content.1,
    )
}

/// 把内容左上角夹进所在显示器的工作区，使整块内容可见。
fn clamp_anchor(anchor: (i32, i32), content: (i32, i32), margin: i32) -> (i32, i32) {
    let work = monitor::work_area_near(POINT {
        x: anchor.0,
        y: anchor.1,
    });
    let x = anchor.0.clamp(
        work.left + margin,
        (work.right - margin - content.0).max(work.left + margin),
    );
    let y = anchor.1.clamp(
        work.top + margin,
        (work.bottom - margin - content.1).max(work.top + margin),
    );
    (x, y)
}

/// 起设置程序；已经在跑就把它叫到前台，不再开第二个。
///
/// 这一步由 Server 做而不是让设置程序自己拦：点齿轮的那一下输入落在这个进程上，
/// Windows 的前台锁才不拦 `SetForegroundWindow`（设置程序自己在后台是抢不到前台的）。
/// 跨进程拿不到对方的窗口句柄，只能枚举顶层窗、按所属进程的 exe 名认。
fn open_settings() {
    if let Some(window) = running_settings() {
        unsafe {
            let _ = ShowWindow(window, SW_RESTORE);
            let _ = SetForegroundWindow(window);
        }
        return;
    }
    let exe = std::env::current_exe().map(|exe| exe.with_file_name("qingjian-settings.exe"));
    let spawned = exe.and_then(|exe| std::process::Command::new(exe).spawn());
    if let Err(error) = spawned {
        tracing::warn!(%error, "打开设置程序失败");
    }
}

/// 已经在跑的设置程序主窗；没在跑返回 `None`。
fn running_settings() -> Option<HWND> {
    let mut found = HWND::default();
    unsafe {
        let _ = EnumWindows(Some(visit_settings_window), LPARAM(&raw mut found as isize));
    }
    (!found.is_invalid()).then_some(found)
}

/// 枚举回调：挑出设置程序那个可见的顶层主窗，找到就停下枚举。
unsafe extern "system" fn visit_settings_window(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    // 不可见的、带主窗的（对话框 / 工具窗）都不是主窗。
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() || unsafe { GetWindow(hwnd, GW_OWNER) }.is_ok() {
        return TRUE;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if !is_settings_process(pid) {
        return TRUE;
    }
    unsafe { *(lparam.0 as *mut HWND) = hwnd };
    FALSE
}

/// `pid` 是不是设置程序（按 exe 文件名认；打不开的进程一律不算）。
fn is_settings_process(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
    else {
        return false;
    };
    let mut buffer = [0u16; 260];
    let mut len = buffer.len() as u32;
    let queried = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    if queried.is_err() {
        return false;
    }
    String::from_utf16_lossy(&buffer[..len as usize])
        .rsplit(['\\', '/'])
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("qingjian-settings.exe"))
}

/// 按下：拖动交给系统移动循环，没拖就是点击；点击不激活；拖动结束报位置。移动只换悬停底色。
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => LRESULT(HTCLIENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            if let Some(bar) = Bar::of(hwnd) {
                let (x, y) = client_pos(lparam);
                bar.on_mouse_move(x, y);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            if let Some(bar) = Bar::of(hwnd) {
                bar.on_mouse_leave();
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let mut point = POINT::default();
            let _ = unsafe { GetCursorPos(&mut point) };
            if unsafe { DragDetect(hwnd, point) }.as_bool() {
                let _ = unsafe { ReleaseCapture() };
                unsafe {
                    SendMessageW(
                        hwnd,
                        WM_NCLBUTTONDOWN,
                        Some(WPARAM(HTCAPTION as usize)),
                        Some(LPARAM(0)),
                    )
                };
            } else if let Some(bar) = Bar::of(hwnd) {
                let (x, y) = client_pos(lparam);
                bar.on_click(x, y);
            }
            LRESULT(0)
        }
        WM_EXITSIZEMOVE => {
            if let Some(bar) = Bar::of(hwnd) {
                bar.on_moved();
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
