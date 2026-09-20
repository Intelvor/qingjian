//! 候选窗口输出：Router 只产出 [`Frame`] 与光标矩形，交给 [`CandidateSink`] 去画；帧没变就不重画。

mod event;
mod sink;

use qingjian_platform::protocol::{Frame, ScreenRect, SessionId};

pub use self::event::CandidateEvent;
pub use self::sink::{CandidateSink, NoopSink, RenderSettings};
use super::Router;

impl Router {
    /// 候选窗口上的鼠标操作（UI 线程点一下，经通道排到工人线程这里消化）。
    pub fn handle_candidate_event(&mut self, event: CandidateEvent) {
        match event {
            CandidateEvent::Pick(row) => self.pick_candidate(row),
            CandidateEvent::PickSentence => match self.sentence.take() {
                // 句子上已经有了：接受它，与按 Tab 同一条路。
                Some(sentence) => {
                    let text = self.engine.accept_prediction(&sentence);
                    self.queue_commit(text);
                }
                // 还没有：现请一次，结果回来那一拍再显示。
                None => {
                    self.request_sentence_now();
                }
            },
        }
    }

    /// 点了本页第 `row` 行：选中它并攒下要上屏的文本。
    ///
    /// Engine 状态立刻前进，所以之后敲的键接在正确状态上；窗口跟着重画——拼音没吃完
    /// （`kaifazhe` 选了 开发，剩 `zhe`）就接着显示后面的候选，吃完了帧变空、窗口收起。
    /// `row` 是页内序号（候选窗手里只有一帧，`page_size` 在这边），按高亮所在页换算成全局下标。
    fn pick_candidate(&mut self, row: usize) {
        let page_size = self.config.page_size;
        let index = (self.highlight / page_size) * page_size + row;
        let Some(text) = self.commit_index(index) else {
            return;
        };
        tracing::debug!(row, index, %text, "候选窗：鼠标点选候选");
        self.queue_commit(text);
    }

    /// 攒下这次要上屏的文本（等 DLL 下一次轮询带回）并按新状态重画候选窗。
    fn queue_commit(&mut self, text: String) {
        // 与按键一样：一次用户动作过去，屏幕提示就该收掉。
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

    /// 取走攒着的上屏文本；只交给持有组句的那个会话（点选只可能落在它身上）。
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
        // 自绘窗吃未降级的帧（降级只作用于发给 DLL 的那份）
        let frame = self.self_drawn_frame();
        self.reconcile_candidates(&frame);
    }
}
