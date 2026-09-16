//! 候选窗口输出：Router 只产出 [`Frame`] 与光标矩形，交给 [`CandidateSink`] 去画；帧没变就不重画。
//! 鼠标点选经 [`CandidateEvent`] 回到这里。

mod event;
mod sink;

use qingjian_platform::protocol::{Frame, ScreenRect, SessionId};

pub use self::event::CandidateEvent;
pub use self::sink::{CandidateSink, NoopSink};
use super::Router;

impl Router {
    /// 候选窗口上的操作（UI 线程点在工人线程的通道里排队，这里消化）。
    pub fn handle_candidate_event(&mut self, event: CandidateEvent) {
        match event {
            CandidateEvent::Pick(row) => self.pick_candidate(row),
            CandidateEvent::PickSentence => self.pick_sentence(),
        }
    }

    /// 点了本页第 `row` 行：选它，上屏文本攒着等 DLL 下一拍轮询来取。
    ///
    /// 文本得由 DLL 写进宿主文档（Server 写不了），而传输一问一答、Server 不能主动推，所以这里
    /// 只把词选掉——Engine 状态立刻前进，之后敲的键接在正确的状态上。窗口跟着重画：拼音没吃完
    /// （`kaifazhe` 选了 开发，剩 `zhe`）就接着显示后面的候选，吃完了帧变空、窗口收起。
    fn pick_candidate(&mut self, row: usize) {
        let page_size = self.config.page_size;
        let index = (self.highlight / page_size) * page_size + row;
        let Some(text) = self.commit_index(index) else {
            return;
        };
        tracing::debug!(row, index, %text, "候选窗：鼠标点选候选");
        self.queue_commit(text);
    }

    /// 点了顶部的整句补全：接受它上屏（与 Tab 键同一条路，整段拼音一次吃完）。
    fn pick_sentence(&mut self) {
        let Some(sentence) = self.sentence.take() else {
            return;
        };
        let text = self.engine.accept_prediction(&sentence);
        tracing::debug!(%text, "候选窗：鼠标点选整句补全");
        self.queue_commit(text);
    }

    /// 攒下这次点选要上屏的文本（等 DLL 下一次轮询带回）并按新状态重画候选窗。
    fn queue_commit(&mut self, text: String) {
        // 与按键一样，一次用户动作过去，屏幕提示就该收掉。
        self.notice = None;
        // 一拍（最迟 80 ms）里连点两次时前一个词还没被取走，接着排后面，别丢。
        match self.pending_commit.take() {
            Some(previous) => self.pending_commit = Some(previous + &text),
            None => self.pending_commit = Some(text),
        }
        self.recompose();
        let frame = self.current_frame();
        self.reconcile_candidates(&frame);
    }

    /// 取走攒着的点选文本；只交给持有组句的那个会话（点选只可能落在它身上）。
    pub(super) fn take_pending_commit(&mut self, session: SessionId) -> Option<String> {
        (self.focused == Some(session))
            .then(|| self.pending_commit.take())
            .flatten()
    }

    /// 空帧收窗口；非空且已知光标矩形就重绘；还没收到矩形（组句刚起）先不显示，免得在旧位置闪一下。
    pub(super) fn reconcile_candidates(&mut self, frame: &Frame) {
        if frame.is_empty() {
            self.engine.note_displayed(std::iter::empty());
            self.hide_candidate_window();
        } else if let Some(rect) = self.last_rect {
            let unchanged = matches!(&self.last_shown, Some((f, r)) if f == frame && *r == rect);
            if !unchanged {
                // 词汇记录的「看到轮次」按真正显示的页算，与 macOS 壳对齐。
                self.engine.note_displayed(frame.candidates.items.iter());
                self.candidates.show(frame.clone(), rect);
                self.last_shown = Some((frame.clone(), rect));
            }
        }
    }

    pub(super) fn hide_candidate_window(&mut self) {
        self.last_rect = None;
        self.last_shown = None;
        self.candidates.hide();
    }

    pub(super) fn position_candidates(&mut self, session: SessionId, rect: ScreenRect) {
        if self.focused != Some(session) {
            return;
        }
        self.last_rect = Some(rect);
        let frame = self.current_frame();
        self.reconcile_candidates(&frame);
    }
}
