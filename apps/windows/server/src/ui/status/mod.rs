//! 悬浮状态条：桌面上常驻、可拖动的四格浮窗 `[中 / 英][，。/ ,.][☁][⚙]`，复用分层窗口合成器与候选窗口主题。
//!
//! 按下鼠标先 `DragDetect`：挪出拖动阈值就交给系统的移动循环（`WM_NCLBUTTONDOWN` + `HTCAPTION`），
//! 结束时 `WM_EXITSIZEMOVE` 报新位置；没挪就是点击，按 x 落进哪格。`WM_MOUSEACTIVATE` 回 `MA_NOACTIVATE` 不抢焦点。
//! 鼠标停在哪一格，那一格铺一层浅底色（与候选窗悬停同一档，见 `candidates/theme`）。
//! 一格的规格在 [`cell`]，动作在 [`action`]。

mod action;
mod cell;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::mem::size_of;
use std::rc::Rc;

use windows::Win32::Foundation::{
    CloseHandle, FALSE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, TRUE, WPARAM,
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
use windows::core::{BOOL, PCWSTR, PWSTR, Result, w};

use qingjian_platform::ThemeMode;

use self::action::StatusAction;
use self::cell::CellSpec;
use super::StatusEvents;
use super::candidates::resolve_dark;
use super::candidates::theme::Theme;
use super::candidates::view;
use super::layered::{self, Layered};
use super::monitor;
use super::window_class::WindowClass;
use crate::dispatch::{StatusEvent, StatusView};

const CLASS_NAME: PCWSTR = w!("QingjianStatusBar");
static CLASS: WindowClass = WindowClass::new();

/// 状态条与屏幕边缘的间隙（逻辑像素）。
const EDGE_GAP: i32 = 8;

thread_local! {
    /// 本线程活着的状态条：HWND → 状态。窗口过程按 HWND 查，查不到（已析构）就忽略。
    static BARS: RefCell<HashMap<isize, Rc<Bar>>> = RefCell::new(HashMap::new());
}

/// 状态条的绘制、摆放与鼠标命中；窗口过程按 HWND 查到它。
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

    /// 上次合成用的阴影留白：拖动结束时从窗口矩形反推内容左上角，点击 / 悬停时把客户区坐标换成内容坐标。
    margin: Cell<i32>,

    /// 内容左上角的屏幕坐标（物理像素）；`None` 表示还没摆放过。
    pos: Cell<Option<(i32, i32)>>,

    /// 上次画出的各格命中带（内容坐标的左右边界，已按悬停底色内缩）与动作，从左到右。
    /// 点得着的地方就是看得见高亮的地方，鼠标挪到条子边缘的留白上就能把高亮收掉。
    bands: RefCell<Vec<(i32, i32, StatusAction)>>,

    /// 命中带的上下范围（内容坐标）；在这外面（条子上下那圈留白）不算落在格子上。
    band_span: Cell<(i32, i32)>,

    /// 鼠标悬停的格号；不在任何格上为 `None`。
    hover: Cell<Option<usize>>,

    /// 已登记 `WM_MOUSELEAVE`；鼠标每次移进窗口登记一次，收到离开后要重登记。
    tracking: Cell<bool>,

    /// 点格 / 拖动结束回给 Router。
    events: StatusEvents,
}

impl Bar {
    /// 建好并登记到本线程的 HWND 表里。
    fn attach(hwnd: HWND, dpi: u32, dark: bool, events: StatusEvents) -> Rc<Self> {
        let bar = Rc::new(Self {
            hwnd,
            data: RefCell::new(None),
            theme: RefCell::new(Rc::new(Theme::new(dpi, dark))),
            dpi: Cell::new(dpi),
            dark: Cell::new(dark),
            margin: Cell::new(layered::shadow_margin(dpi)),
            pos: Cell::new(None),
            bands: RefCell::new(Vec::new()),
            band_span: Cell::new((0, 0)),
            hover: Cell::new(None),
            tracking: Cell::new(false),
            events,
        });
        BARS.with(|map| map.borrow_mut().insert(hwnd.0 as isize, bar.clone()));
        bar
    }

    /// 窗口析构时摘掉，免得窗口过程拿到已经不画的那个。
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
        let _ = unsafe { ShowWindow(self.hwnd, SW_HIDE) };
    }

    /// DPI 或深浅变了就重建主题。
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
        if dpi != self.dpi.get() || dark != self.dark.get() {
            *self.theme.borrow_mut() = Rc::new(Theme::new(dpi, dark));
            self.dpi.set(dpi);
            self.dark.set(dark);
        }
    }

    /// 四格从左到右：模式（品牌色）、标点（生效时品牌色，否则灰）、云联想（开着品牌色，否则灰）、齿轮（灰）。
    fn cells(&self, theme: &Theme) -> Vec<CellSpec> {
        let data = self.data.borrow();
        let Some(view) = data.as_ref() else {
            return Vec::new();
        };
        let mode = if view.english {
            "英".to_owned()
        } else if view.zhuyin {
            "注".to_owned()
        } else {
            match &view.scheme {
                Some(scheme) => format!("中 · {scheme}"),
                None => "中".to_owned(),
            }
        };
        let punctuation_active = view.full_width;
        vec![
            CellSpec {
                text: mode,
                font: theme.text_font,
                color: theme.cloud_color,
                action: StatusAction::ToggleMode,
            },
            CellSpec {
                text: if punctuation_active { "，。" } else { ",." }.to_owned(),
                font: theme.text_font,
                color: if punctuation_active {
                    theme.cloud_color
                } else {
                    theme.gloss_color
                },
                action: StatusAction::TogglePunctuation,
            },
            CellSpec {
                text: "\u{2601}".to_owned(),
                font: theme.symbol_font,
                color: if view.cloud {
                    theme.cloud_color
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
    }

    /// 量各格、算内容尺寸、摆位置、合成贴上；顺带记下各格边界给点击 / 悬停用。
    fn render(&self) {
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
        // 每格：文字左右各留一个半 padding（悬停底色内缩半个 padding，两侧正好让出格间那条细线），
        // 格与格之间一条细线。
        let widths: Vec<i32> = sizes
            .iter()
            .map(|size| size.cx + theme.padding * 3)
            .collect();
        let content = (widths.iter().sum::<i32>(), line + theme.padding);
        if content.0 <= 0 || content.1 <= 0 || cells.is_empty() {
            self.hide();
            return;
        }
        // 命中带与悬停底色同一块（左右各内缩半个 padding，上下也留一点）：看得见的是这儿，
        // 点得着的也是这儿；鼠标挪到条子边缘那圈留白上，高亮就该消失。
        let inset = theme.padding / 2;
        self.band_span.set((inset / 2, content.1 - inset / 2));
        let mut x = 0;
        let bands: Vec<(i32, i32, StatusAction)> = cells
            .iter()
            .zip(&widths)
            .map(|(cell, width)| {
                let band = (x + inset, x + width - inset, cell.action);
                x += width;
                band
            })
            .collect();
        *self.bands.borrow_mut() = bands;

        let anchor = self
            .pos
            .get()
            .unwrap_or_else(|| default_anchor(content, margin));
        let anchor = clamp_anchor(anchor, content, margin);
        self.pos.set(Some(anchor));

        let hover = self.hover.get();
        let updated = layered::composite(
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
                    paint_cells(hdc, client, &cells, &sizes, &widths, &theme, hover);
                },
            },
        );
        if updated.is_ok() {
            let _ = unsafe { ShowWindow(self.hwnd, SW_SHOWNA) };
        } else {
            self.hide();
        }
        // 窗口换了位置时鼠标底下那一格可能变了（拖动结束后）。
        self.refresh_hover();
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
    fn on_click(&self, client_x: i32, client_y: i32) {
        let action = self
            .cell_at(client_x, client_y)
            .and_then(|index| self.bands.borrow().get(index).map(|(_, _, action)| *action));
        match action {
            Some(StatusAction::ToggleMode) => (self.events)(StatusEvent::ToggleMode),
            Some(StatusAction::TogglePunctuation) => (self.events)(StatusEvent::TogglePunctuation),
            Some(StatusAction::ToggleCloud) => (self.events)(StatusEvent::ToggleCloud),
            Some(StatusAction::OpenSettings) => open_settings(),
            None => {}
        }
    }

    /// 鼠标移到客户区 `(x, y)`：落在哪一格就高亮哪一格，落到条子边缘的留白上就收掉。
    fn on_mouse_move(&self, x: i32, y: i32) {
        self.track_leave();
        let hover = self.cell_at(x, y);
        if hover != self.hover.get() {
            self.hover.set(hover);
            self.render();
        }
    }

    /// 鼠标离开窗口：收掉悬停底色。
    fn on_mouse_leave(&self) {
        self.tracking.set(false);
        if self.hover.replace(None).is_some() {
            self.render();
        }
    }

    /// 按鼠标当前位置重算悬停（窗口刚摆好 / 拖完，鼠标没动时收不到消息）。
    fn refresh_hover(&self) {
        let mut point = POINT::default();
        if unsafe { GetCursorPos(&mut point) }.is_err()
            || !unsafe { ScreenToClient(self.hwnd, &mut point) }.as_bool()
        {
            return;
        }
        let hover = self.cell_at(point.x, point.y);
        if hover != self.hover.get() {
            self.hover.set(hover);
            self.render();
        }
    }

    /// 客户区坐标落在第几格上；条子上下那圈留白不算。
    fn cell_at(&self, x: i32, y: i32) -> Option<usize> {
        let (x, y) = (x - self.margin.get(), y - self.margin.get());
        let (top, bottom) = self.band_span.get();
        if y < top || y >= bottom {
            return None;
        }
        self.bands
            .borrow()
            .iter()
            .position(|(left, right, _)| x >= *left && x < *right)
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

/// 悬浮状态条窗口。
pub(super) struct StatusBar {
    /// 绘制、摆放与命中状态；窗口过程按 HWND 查到同一份。
    bar: Rc<Bar>,
}

impl StatusBar {
    /// 建一个隐藏的状态条窗口。
    pub(super) fn new(events: StatusEvents) -> Result<Self> {
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
            bar: Bar::attach(hwnd, dpi, dark, events),
        })
    }

    /// 显示 / 更新。
    pub(super) fn update(&self, view: StatusView) {
        self.bar.update(view);
    }

    pub(super) fn hide(&self) {
        self.bar.hide();
    }
}

impl Drop for StatusBar {
    fn drop(&mut self) {
        Bar::detach(self.bar.hwnd);
        let _ = unsafe { DestroyWindow(self.bar.hwnd) };
    }
}

/// 每格文字居中；格与格之间一条上下留 `inset` 的细线；鼠标停住的那格先铺一层浅底色。
/// 悬停底色左右各内缩 `inset`，免得盖住两侧的分隔线（格子宽出的一圈正是留给它的）。
fn paint_cells(
    hdc: HDC,
    client: RECT,
    cells: &[CellSpec],
    sizes: &[SIZE],
    widths: &[i32],
    theme: &Theme,
    hover: Option<usize>,
) {
    let separator = theme.pos_color;
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
                separator,
            );
        }
        if hover == Some(index) {
            view::fill_round_rect(
                hdc,
                RECT {
                    left: x + inset,
                    top: inset / 2,
                    right: x + width - inset,
                    bottom: client.bottom - inset / 2,
                },
                theme.hover,
                theme.corner_radius / 2,
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

/// 起与本 exe 同目录的设置程序；已经在跑就把它叫到前台，不再开第二个。
///
/// 跨进程拿不到对方的窗口句柄，只能枚举顶层窗、按所属进程的 exe 名认。这一步由 Server 做而不是让设置程序
/// 自己单例：点齿轮的那一下输入落在这个进程上，`SetForegroundWindow` 才不会被 Windows 的前台锁拦下来。
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
unsafe extern "system" fn visit_settings_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
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

/// 按下：拖动交给系统移动循环，没拖就是点击；点击不激活；拖动结束报位置。移动只是换悬停底色。
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
