//! 轮询定时器：组句期间每隔一小段时间向 Server 拉一次异步结果（云端候选 / 整句补全）；
//! 没在组句、本线程在前台时，隔几拍问一次状态条上有没有点出切模式的请求（中英模式在 DLL 侧，Server 只能等我们来取）。
//! 云端结果几百毫秒后才回，那时往往没有新按键来「顺手收一次」，所以在 TSF 线程上挂一个 `WM_TIMER`；
//! 传输仍是一问一答。定时器挂在隐藏的消息窗口上，与按键同在 STA 消息泵上跑。回调上下文在 [`context`]。

mod context;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_MESSAGE, KillTimer, SetTimer,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_TIMER, WNDCLASSEXW,
};
use windows::core::{Error, PCWSTR, Result, w};

use self::context::{PollContext, should_sync_mode};
use super::composition::Shared;
use super::log::log;
use super::service::SharedClient;
use super::window_class::WindowClass;

const CLASS_NAME: PCWSTR = w!("QingjianPollWindow");
static CLASS: WindowClass = WindowClass::new();

const TIMER_ID: usize = 1;
const INTERVAL_MS: u32 = 80;

/// 每几拍问一次状态条的切模式请求（320 ms 一次，点了状态条肉眼看不出延迟）。组句中也照问，
/// 否则点了状态条再开始打字，那个目标模式会一直挂在 Server 那边等不到取回。
/// 按键行为设置（切换键、内置英文模式）也跟着这一拍取回，所以设置改完同样是约 320 ms 生效。
const MODE_SYNC_EVERY: u32 = 4;

thread_local! {
    /// 本线程活着的定时器：消息窗口 → 回调上下文。查不到（已析构）就忽略这一拍。
    static TIMERS: RefCell<HashMap<isize, Rc<PollContext>>> = RefCell::new(HashMap::new());
}

/// 承载轮询定时器的隐藏消息窗口；`Drop` 里停表、销毁窗口、注销上下文。
pub(crate) struct PollTimer {
    hwnd: HWND,
}

impl PollTimer {
    /// 失败返回 `Err`，调用方降级为只在按键时收云结果。
    pub(crate) fn new(engine: SharedClient, shared: Rc<Shared>) -> Result<Self> {
        CLASS.ensure(|| WNDCLASSEXW {
            lpfnWndProc: Some(wndproc),
            hInstance: super::dll_instance(),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        })?;
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS_NAME,
                w!(""),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(super::dll_instance()),
                None,
            )?
        };
        let timer = Self { hwnd };
        if unsafe { SetTimer(Some(hwnd), TIMER_ID, INTERVAL_MS, None) } == 0 {
            return Err(Error::from(E_FAIL));
        }
        TIMERS.with(|timers| {
            timers.borrow_mut().insert(
                hwnd.0 as isize,
                Rc::new(PollContext {
                    engine,
                    shared,
                    ticks: Cell::new(0),
                }),
            )
        });
        Ok(timer)
    }
}

impl Drop for PollTimer {
    fn drop(&mut self) {
        TIMERS.with(|timers| timers.borrow_mut().remove(&(self.hwnd.0 as isize)));
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_ID);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_TIMER {
        // 先 clone 出来放开表的借用。
        let context = TIMERS.with(|timers| timers.borrow().get(&(hwnd.0 as isize)).cloned());
        if let Some(context) = context {
            // 从消息泵调进来：panic 不能越过 FFI。
            let _ = catch_unwind(AssertUnwindSafe(|| poll_once(&context)));
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// 组句中或翻译评审中拉云结果；每隔几拍问一次切模式（顺路取回按键行为设置）。引擎正被按键处理借用时跳过这一拍；连接坏了断开。
fn poll_once(context: &PollContext) {
    let tick = context.ticks.get().wrapping_add(1);
    context.ticks.set(tick);
    let translating = context.shared.translating();
    let composing = context.shared.composing();
    if should_sync_mode(context) {
        sync_mode(context);
    }
    if !composing && !translating {
        return;
    }
    let Ok(mut guard) = context.engine.try_borrow_mut() else {
        return;
    };
    let Some(client) = guard.as_mut() else {
        return;
    };
    match client.poll() {
        Ok(reply) => {
            // 后面要借引擎走编辑会话 / 通知 Server，先把这一拍的借用放掉。
            drop(guard);
            // 翻译评审时回空帧 = 翻译已在 Server 侧结束（云端没给译文）。
            let ended = translating && reply.frame.is_empty();
            // 用户在候选窗点了候选：Server 已经把词选上，这里把它落进文档。
            if let Some(text) = reply.commit {
                super::service::on_pick_commit(text, reply.frame);
            }
            if ended {
                context.shared.set_translating(false);
                context.shared.hide_candidates();
            }
        }
        Err(error) => {
            log(&format!("云联想轮询失败，断开，下一键重连: {error}"));
            *guard = None;
            context.shared.end_composing();
        }
    }
}

/// 取一次状态条上点出的目标模式，顺路取回 Server 下发的按键行为设置。切模式会回报 Server、要借引擎，
/// 所以先放掉借用再切。
fn sync_mode(context: &PollContext) {
    if !should_sync_mode(context) {
        return;
    }
    let Ok(mut guard) = context.engine.try_borrow_mut() else {
        return;
    };
    let Some(client) = guard.as_mut() else {
        return;
    };
    let reply = match client.sync_mode() {
        Ok(reply) => reply,
        Err(error) => {
            log(&format!("同步中英模式失败，断开，下一键重连: {error}"));
            *guard = None;
            return;
        }
    };
    drop(guard);
    // 顺手把本应用当前的中英模式推给 Server（悬浮状态条用）。Chromium 系（Edge / Chrome / Electron）
    // 切回来时不触发 TSF 的 OnSetFocus，只靠那条路的话状态条会停在别的应用的模式上。这一拍只在
    // 「本应用是前台」时才跑（见 should_sync_mode），所以推上去的就是前台应用的模式。
    if let Some(english) = super::service::current_english()
        && let Some(client) = context.engine.borrow_mut().as_mut()
        && let Err(error) = client.mode_changed(english)
    {
        log(&format!("上报中英模式失败: {error}"));
    }
    // 按键行为设置每一拍都带（DLL 不读配置文件），切换键与内置英文模式开关改完靠它生效。
    super::service::on_input_settings(reply.input);
    if let Some(english) = reply.english {
        super::service::on_mode_sync(english);
    }
}
