//! 当前中 / 英模式与语言栏更新回调，文本服务与语言栏按钮（[`super::ModeButton`]）共享。

use core::cell::{Cell, RefCell};
use std::rc::Rc;

use windows::Win32::UI::TextServices::{ITfLangBarItemSink, TF_LBI_ICON, TF_LBI_STATUS};

use qingjian_platform::SwitchKey;

/// 当前中英模式 + 语言栏更新回调，文本服务与语言栏按钮共享（STA 单线程）。
pub(crate) struct ModeState {
    /// `true` 是英文模式。
    english: Cell<bool>,

    /// 内置英文模式开关（`[general] english_mode`）：关掉后谁都不许切到英文。
    enabled: Cell<bool>,

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

    pub(crate) fn switch_key(&self) -> SwitchKey {
        self.switch_key.get()
    }

    /// 激活时按配置设一次。
    pub(crate) fn set_settings(&self, enabled: bool, switch_key: SwitchKey) {
        self.enabled.set(enabled);
        self.switch_key.set(switch_key);
    }

    /// 通知系统重取图标 / 文字。
    pub(crate) fn notify(&self) {
        if let Some(sink) = self.sink.borrow().as_ref() {
            let _ = unsafe { sink.OnUpdate(TF_LBI_ICON | TF_LBI_STATUS) };
        }
    }
}
