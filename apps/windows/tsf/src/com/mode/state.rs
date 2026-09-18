//! 当前中 / 英模式与语言栏更新回调，文本服务与语言栏按钮（[`super::ModeButton`]）共享。

use core::cell::{Cell, RefCell};
use std::rc::Rc;

use windows::Win32::UI::TextServices::{ITfLangBarItemSink, TF_LBI_ICON, TF_LBI_STATUS};

use qingjian_platform::SwitchKey;
use qingjian_platform::protocol::ModeGlyph;

/// 当前中英模式 + 语言栏更新回调，文本服务与语言栏按钮共享（STA 单线程）。
pub(crate) struct ModeState {
    /// `true` 是英文模式。
    english: Cell<bool>,

    /// 内置英文模式开关（`[general] english_mode`）：关掉后谁都不许切到英文。
    enabled: Cell<bool>,

    /// 任务栏那张图标画哪个字（拼音侧方案 + 五笔，由 Server 按优先级挑一个下发，见 [`ModeGlyph`]）。
    glyph: Cell<ModeGlyph>,

    /// 这次（这个应用）给不给英文候选（`[general] english_candidates` + `[apps] english_candidates_off`）。
    /// 关着时英文模式**一个键都不吃**，字母 / 数字 / 标点原样进应用（打游戏那种），
    /// 判定见 `service::key_sink::eats_key`；中英切换键不受影响（走 `key_tap` 那条独立路径）。
    english_candidates: Cell<bool>,

    /// 中英切换键（`[shortcut] switch_mode`），单击判定与语言栏提示用。
    switch_key: Cell<SwitchKey>,

    /// 系统登记进来的语言栏更新回调；由 [`super::ModeButton`] 的 `ITfSource` 登记 / 撤销。
    pub(super) sink: RefCell<Option<ITfLangBarItemSink>>,
}

impl ModeState {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self {
            english: Cell::new(false),
            enabled: Cell::new(true),
            glyph: Cell::new(ModeGlyph::default()),
            english_candidates: Cell::new(true),
            switch_key: Cell::new(SwitchKey::default()),
            sink: RefCell::new(None),
        })
    }

    pub(crate) fn english(&self) -> bool {
        self.english.get()
    }

    pub(crate) fn set_english(&self, english: bool) {
        self.english.set(english);
    }

    /// 内置英文模式是否可用。
    pub(crate) fn enabled(&self) -> bool {
        self.enabled.get()
    }

    /// 任务栏那张图标画哪个字。
    pub(crate) fn glyph(&self) -> ModeGlyph {
        self.glyph.get()
    }

    /// 这次（这个应用）给不给英文候选。关着时英文模式直接放行所有键。
    pub(crate) fn english_candidates(&self) -> bool {
        self.english_candidates.get()
    }

    pub(crate) fn switch_key(&self) -> SwitchKey {
        self.switch_key.get()
    }

    /// 激活时按配置设一次；返回**图标要不要重取**（换成别的方案 / 开关五笔时得通知系统，
    /// 否则要等到下次切模式才刷新）。
    pub(crate) fn set_settings(
        &self,
        enabled: bool,
        switch_key: SwitchKey,
        glyph: ModeGlyph,
        english_candidates: bool,
    ) -> bool {
        self.enabled.set(enabled);
        self.switch_key.set(switch_key);
        self.english_candidates.set(english_candidates);
        let changed = self.glyph.get() != glyph;
        self.glyph.set(glyph);
        changed
    }

    /// 通知系统重取图标 / 文字。
    pub(crate) fn notify(&self) {
        if let Some(sink) = self.sink.borrow().as_ref() {
            let _ = unsafe { sink.OnUpdate(TF_LBI_ICON | TF_LBI_STATUS) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 任务栏那格字只在**变了**的时候才算「要刷新图标」：Server 每一拍 `SyncMode` 都带着这份设置，
    /// 不变还去 `notify()` 就是白让系统重取图标。
    #[test]
    fn settings_report_only_a_glyph_change() {
        let state = ModeState::new();
        assert_eq!(state.glyph(), ModeGlyph::Pinyin, "缺省是全拼的「拼」");
        assert!(
            state.set_settings(true, SwitchKey::Shift, ModeGlyph::Zhuyin, true),
            "换方案要刷新图标"
        );
        assert_eq!(state.glyph(), ModeGlyph::Zhuyin);
        assert!(
            !state.set_settings(true, SwitchKey::Shift, ModeGlyph::Zhuyin, true),
            "值没变不用刷新"
        );
        assert!(
            state.set_settings(true, SwitchKey::Shift, ModeGlyph::Wubi, false),
            "开五笔也要刷新"
        );
        assert_eq!(state.glyph(), ModeGlyph::Wubi);
    }
}
