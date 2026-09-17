//! 文本服务对象 [`TextService`]：每线程一个，实现 `ITfTextInputProcessor`（激活 / 停用，[`processor`]）、
//! `ITfKeyEventSink`（收键，[`key_sink`]）与显示属性提供者（[`display`]）。
//! 连 Server 在 [`connection`]，中英模式在 [`mode`]，往文档写字在 [`document`]。

mod connection;
mod display;
mod document;
mod key_sink;
mod launch;
mod mode;
mod next;
mod processor;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use windows::Win32::UI::TextServices::{
    ITfDisplayAttributeProvider, ITfKeyEventSink, ITfLangBarItemButton, ITfSource,
    ITfTextInputProcessor, ITfThreadMgr,
};
use windows::core::{ComObject, implement};

use qingjian_platform::KeyCombo;
use qingjian_platform::protocol::{Frame, InputSettings};

use super::composition::Shared;
use super::key::KeyTap;
use super::mode::ModeState;
use super::poll::PollTimer;
use crate::client::EngineClient;
use crate::client::pipe::PipeStream;

/// 连 Server 的会话客户端，与编辑会话 / 轮询定时器共享（STA 单线程）。连不上时为 `None`，键照样放行。
pub(crate) type SharedClient = Rc<RefCell<Option<EngineClient<PipeStream>>>>;

/// 连不上 Server 后隔多久再试（每次尝试都在应用的 UI 线程上，不能每键都试）。
const RECONNECT_INTERVAL: Duration = Duration::from_secs(2);

/// 一个 TSF 文本服务实例（每线程一个）。
#[implement(ITfTextInputProcessor, ITfKeyEventSink, ITfDisplayAttributeProvider)]
pub struct TextService {
    /// 激活时拿到的线程管理器，停用时用它反注册。
    thread_mgr: RefCell<Option<ITfThreadMgr>>,

    /// TSF 分配的 client id。
    client_id: Cell<u32>,

    /// 引擎层。
    engine: SharedClient,

    /// 跨按键存活的组句状态。
    shared: Rc<Shared>,

    /// 云联想轮询定时器；挂失败时为 `None`，退化为只在按键时收云结果。
    poll_timer: RefCell<Option<PollTimer>>,

    /// 上次连 Server 失败的时间，按 [`RECONNECT_INTERVAL`] 退避。
    last_connect_failure: Cell<Option<Instant>>,

    /// 中 / 英模式（单击切换键翻转，见 `[shortcut] switch_mode`），与语言栏按钮共用。
    mode_state: Rc<ModeState>,

    /// 登记在系统语言栏上的中 / 英按钮；停用时反注册。
    mode_button: RefCell<Option<ITfLangBarItemButton>>,

    /// 「转换模式」compartment 的事件回调（source + cookie），反向同步任务栏点选；停用时撤掉。
    conversion_sink: RefCell<Option<(ITfSource, u32)>>,

    /// 单击中英切换键切中英的判定。
    key_tap: KeyTap,

    /// 语言 profile 通知挂上后的 cookie；挂一次就够（见 [`super::profile`]）。
    profile_cookie: Cell<Option<u32>>,

    /// 登记成保留键的「翻译选中文字」组合；停用时撤掉（见 [`preserved`](crate::com::key::preserved)）。
    translate_combo: Cell<Option<KeyCombo>>,

    /// 上一次应用过的按键行为设置；与 Server 下发的一致时就不重复应用
    /// （每一拍 `SyncMode` 都带着它，见 [`TextService_Impl::apply_input_settings`]）。
    input_settings: Cell<Option<InputSettings>>,

    /// 激活后一小段时间内忽略转换模式 compartment 的变化，见 [`TextService_Impl::sync_from_conversion_mode`]。
    conversion_guard_until: Cell<Option<Instant>>,
}

thread_local! {
    /// 本线程当前激活的文本服务，供转换模式回调 / 轮询定时器切模式。`Activate` 设、`Deactivate` 清。
    static ACTIVE: RefCell<Option<ComObject<TextService>>> = const { RefCell::new(None) };
}

fn with_active(f: impl FnOnce(&TextService_Impl)) {
    let service = ACTIVE.with(|active| active.borrow().clone());
    if let Some(service) = service {
        f(&service);
    }
}

/// 前台窗口属于本应用吗？中英模式的上报与「要不要取状态条上点的切换」都看它。
///
/// 两个条件满足其一：
/// 1. 前台窗口就在本进程里 —— 记事本、画图这类一个应用一个进程的走这条；
/// 2. 前台进程是本进程的子孙 —— Chromium 系（Edge / Chrome）的前台窗口属于它的 renderer 子进程，
///    而 TSF 在浏览器进程里，父链上有「我」。
///
/// **不能只比 exe 名**：开两个记事本时两边 exe 名相同，会比出「两个都在前台」，状态条就横跳。
pub(super) fn is_foreground_app() -> bool {
    let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    if hwnd.0.is_null() {
        return false;
    }
    let mut pid = 0u32;
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    if pid == 0 {
        return false;
    }
    pid == std::process::id() || ancestor_is_me(pid)
}

/// `pid` 的祖先里有本进程？（Chromium 的前台窗口在 renderer，往上的父链里就是浏览器进程。）
fn ancestor_is_me(mut pid: u32) -> bool {
    let me = std::process::id();
    // 最多上溯几层，顺便防住意外成环
    for _ in 0..8 {
        let Some(parent) = parent_pid(pid) else {
            return false;
        };
        if parent == me {
            return true;
        }
        if parent == 0 || parent == pid {
            return false;
        }
        pid = parent;
    }
    false
}

/// `pid` 的父进程 id。
fn parent_pid(pid: u32) -> Option<u32> {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.ok()?;
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut walking = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    let mut found = None;
    while walking {
        if entry.th32ProcessID == pid {
            found = Some(entry.th32ParentProcessID);
            break;
        }
        walking = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    let _ = unsafe { windows::Win32::Foundation::CloseHandle(snapshot) };
    found
}

/// 本线程激活的文本服务当前的中英模式；没激活时为 `None`。轮询那一拍用它把模式推给 Server。
pub(super) fn current_english() -> Option<bool> {
    ACTIVE.with(|active| {
        active
            .borrow()
            .as_ref()
            .map(|service| service.mode_state.english())
    })
}

/// 用户点了语言栏的中 / 英按钮（见 [`ModeButton`](crate::com::mode::ModeButton)）：翻转模式。
pub(super) fn toggle_mode() {
    with_active(|service| service.set_english_mode(!service.mode_state.english()));
}

/// 「转换模式」compartment 变了（见 [`conversion`](crate::com::mode::conversion)）。
pub(super) fn on_conversion_mode_changed() {
    with_active(TextService_Impl::sync_from_conversion_mode);
}

/// 轮询取到了状态条上点出的目标模式（见 [`super::poll`]）；与当前相同就不动。
pub(super) fn on_mode_sync(english: bool) {
    with_active(|service| {
        if service.mode_state.english() != english {
            service.set_english_mode(english);
        }
    });
}

/// 轮询取回了 Server 下发的按键行为设置（见 [`super::poll`]）：切换键 / 内置英文模式改了就地应用。
pub(super) fn on_input_settings(input: InputSettings) {
    with_active(|service| service.apply_input_settings(input));
}

/// 轮询取到了候选窗上点选的候选（见 [`super::poll`]）：把 Server 选好的词落进文档。
pub(super) fn on_pick_commit(commit: String, frame: Frame) {
    with_active(|service| service.commit_picked(commit, frame));
}

impl TextService {
    #[allow(clippy::new_without_default)] // 有 lock_module 副作用
    pub fn new() -> Self {
        crate::com::lock_module();
        let engine: SharedClient = Rc::new(RefCell::new(None));
        Self {
            thread_mgr: RefCell::new(None),
            client_id: Cell::new(0),
            shared: Shared::new(engine.clone()),
            engine,
            poll_timer: RefCell::new(None),
            last_connect_failure: Cell::new(None),
            mode_state: ModeState::new(),
            mode_button: RefCell::new(None),
            conversion_sink: RefCell::new(None),
            key_tap: KeyTap::default(),
            profile_cookie: Cell::new(None),
            translate_combo: Cell::new(None),
            input_settings: Cell::new(None),
            conversion_guard_until: Cell::new(None),
        }
    }
}

impl Drop for TextService {
    fn drop(&mut self) {
        crate::com::unlock_module();
    }
}
