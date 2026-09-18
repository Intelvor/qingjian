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

use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::TextServices::{
    ITfDisplayAttributeProvider, ITfKeyEventSink, ITfLangBarItemButton, ITfSource,
    ITfTextInputProcessor, ITfThreadMgr,
};
use windows::core::{ComObject, implement};

use qingjian_platform::KeyCombo;
use qingjian_platform::protocol::{Frame, InputSettings, SessionId};

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

/// 本线程的 `(进程 id, 线程 id)`。
pub(super) fn host_ids() -> (u32, u32) {
    unsafe { (GetCurrentProcessId(), GetCurrentThreadId()) }
}

/// 本线程的会话标识 = 进程号 `<< 32 | 线程号`。**调它的必须是承载这个会话的那条线程**
/// （`Activate` 与 `connect` 都在那条 STA 线程上）。
///
/// 不能拿 `Activate` 传进来的 client id 当会话标识：实测那几个值（0 / 25 / 57）在完全不同的进程之间
/// 反复出现，而 Server 的会话表是全局一张（一个 Server 服务所有应用），于是后开的会话把先开的顶掉 ——
/// 表现出来是 `[apps]` 按应用的设置认错应用、两个应用的中英模式互相覆盖（「切应用后状态条中 / 英乱跳」）。
/// 进程号 + 线程号一起才在「活着的进程」里唯一。
pub(super) fn session_id() -> SessionId {
    let (pid, tid) = host_ids();
    SessionId((u64::from(pid) << 32) | u64::from(tid))
}

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

    /// 激活后一小段时间内忽略转换模式 compartment 的变化（msctf 会把线程 profile 里记的模式写回来，
    /// 那不是用户操作），见 [`TextService_Impl::start_mode_from_settings`] 与 `sync_from_conversion_mode`。
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
