//! 轮询定时器每一拍回调要用的东西：按消息窗口句柄从线程表里查出来。

use std::cell::Cell;
use std::rc::Rc;

use crate::com::composition::Shared;
use crate::com::service::SharedClient;

pub(super) struct PollContext {
    /// 引擎层：拉云结果 / 问切模式请求。
    pub(super) engine: SharedClient,

    /// 组句状态：在不在组句 / 翻译评审 / 前台。
    pub(super) shared: Rc<Shared>,

    /// 拍数计数，给 [`MODE_SYNC_EVERY`](super::MODE_SYNC_EVERY) 取模。
    pub(super) ticks: Cell<u32>,
}

/// 这一拍该不该向 Server 问一次切模式请求。
///
/// 状态条常驻桌面，与正在输入的应用无关；失焦 / 没组句 / 没翻译评审时仍要按节拍问，
/// 否则点击状态条后切到别的应用再切回来，模式要等一拍轮询才能恢复。
///
/// 节拍按 `MODE_SYNC_EVERY`（约 320 ms）做一次，避免每次轮询都来回打管道。
pub(super) fn should_sync_mode(context: &PollContext) -> bool {
    context
        .ticks
        .get()
        .is_multiple_of(crate::com::poll::MODE_SYNC_EVERY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com::composition::Shared;
    use crate::com::service::SharedClient;
    use std::cell::RefCell;

    /// 构造最小可用的 `PollContext`：只测拍数决策，不碰真实引擎与组句状态。
    fn fixture(tick: u32) -> PollContext {
        let client: SharedClient = Rc::new(RefCell::new(None));
        PollContext {
            engine: client,
            shared: Shared::new(Rc::new(RefCell::new(None))),
            ticks: Cell::new(tick),
        }
    }

    #[test]
    fn fires_on_every_mode_sync_beat() {
        for tick in (0..32u32).map(|n| n * crate::com::poll::MODE_SYNC_EVERY) {
            assert!(should_sync_mode(&fixture(tick)), "tick={tick} should fire");
        }
    }

    #[test]
    fn skips_between_beats() {
        let beat = crate::com::poll::MODE_SYNC_EVERY;
        for offset in 1..beat {
            assert!(
                !should_sync_mode(&fixture(offset)),
                "offset={offset} should not fire"
            );
        }
    }

    /// 回归用例：状态条常驻桌面时 DLL 失焦 / 没组句 / 没翻译评审都要按节拍问 Server。
    /// 决策器只读拍数、不依赖 `shared`，避免历史上「(composing || foreground)」前提把这条漏掉。
    #[test]
    fn independent_of_shared_state() {
        let beat = crate::com::poll::MODE_SYNC_EVERY;
        let context = fixture(beat);
        // 组句 = false（fixture 默认）、前台 = false（fixture 默认）；仍要 fire。
        assert!(should_sync_mode(&context));
    }
}
