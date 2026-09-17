//! 中 / 英模式：切模式先把组着的内容落定；指示器走语言栏按钮 + 转换模式 compartment，并推给 Server 的状态条；
//! 用户点任务栏中 / 英时由 compartment 回调反向同步。切换键与内置英文模式开关由 Server 经协议下发
//! （[`TextService_Impl::apply_input_settings`]），DLL 不读配置文件。

use std::time::Instant;

use windows::Win32::UI::TextServices::ITfLangBarItemMgr;
use windows::core::Interface;

use qingjian_platform::SwitchKey;
use qingjian_platform::protocol::InputSettings;

use super::TextService_Impl;
use crate::com::log::log;
use crate::com::mode::{self, ModeButton, conversion};

impl TextService_Impl {
    /// 应用中英模式的两项设置：激活时与配置变更时都走这里。
    pub(super) fn apply_mode_settings(&self, english_mode: bool, switch_key: SwitchKey) {
        self.mode_state.set_settings(english_mode, switch_key);
        // 关掉内置英文模式时立刻回中文，别停在一个再也切不回去的英文状态。
        if !english_mode && self.mode_state.english() {
            self.mode_state.set_english(false);
            self.refresh_mode_indicator();
        }
    }

    /// 应用 Server 下发的按键行为设置：`OpenSession` 的回包给一次，之后每一拍 `SyncMode` 也都带着。
    /// 值没变就什么都不做，所以设置窗口改完在下一拍（约 320 ms）生效，不用切走再切回输入法。
    pub(super) fn apply_input_settings(&self, input: InputSettings) {
        if self.input_settings.get() == Some(input) {
            return;
        }
        self.input_settings.set(Some(input));
        log(&format!(
            "按键行为设置：中英切换键 {}，内置英文模式 {}",
            input.switch_mode.key(),
            input.english_mode
        ));
        self.apply_mode_settings(input.english_mode, input.switch_mode);
    }

    /// 切模式：先把组着的内容原样落定，再刷指示器。
    ///
    /// 配置关掉了内置英文模式时这里什么都不做——切换键、语言栏按钮、悬浮状态条、任务栏转换模式四条入口
    /// 都汇到这里，一处拦住就再也进不了英文模式（见 issue #81）。
    pub(super) fn set_english_mode(&self, english: bool) {
        if !self.mode_state.enabled() {
            if english {
                log("内置英文模式已关闭，忽略切到英文");
            }
            return;
        }
        self.commit_pending();
        self.mode_state.set_english(english);
        self.refresh_mode_indicator();
        log(if english {
            "切到英文模式"
        } else {
            "切到中文模式"
        });
    }

    /// 激活 / 切到本应用后的一小段时间里，msctf 会把 profile 存着的转换模式写回来，那不是用户操作。
    /// 采纳它的话，切到每个应用都会被打回上次那个模式（表现：一换应用中英模式就自己变），
    /// 而状态条显示的是 DLL 报上来的模式，看起来就像「状态条没跟上」。这段时间内忽略写入。
    pub(super) fn guard_conversion_mode(&self) {
        self.conversion_guard_until
            .set(Some(Instant::now() + std::time::Duration::from_millis(500)));
    }

    /// 语言栏按钮换图标、写转换模式 compartment、把模式推给 Server（悬浮状态条）。
    pub(super) fn refresh_mode_indicator(&self) {
        let english = self.mode_state.english();
        self.mode_state.notify();
        if let Some(thread_mgr) = self.thread_mgr.borrow().as_ref() {
            mode::set_indicator(thread_mgr, self.client_id.get(), english);
        }
        if let Some(client) = self.engine.borrow_mut().as_mut()
            && let Err(error) = client.mode_changed(english)
        {
            log(&format!("上报中英模式失败: {error}"));
        }
    }

    pub(super) fn advise_conversion_sink(&self) {
        let Some(thread_mgr) = self.thread_mgr.borrow().clone() else {
            return;
        };
        match conversion::advise(&thread_mgr) {
            Ok(advice) => *self.conversion_sink.borrow_mut() = Some(advice),
            Err(error) => log(&format!("监听转换模式失败: {error}")),
        }
    }

    pub(super) fn unadvise_conversion_sink(&self) {
        if let Some((source, cookie)) = self.conversion_sink.borrow_mut().take() {
            conversion::unadvise(&source, cookie);
        }
    }

    /// 用户在任务栏点了中 / 英：读回 `NATIVE` 位，与当前不同才切（相同是自己那次写触发的，防回环）。
    ///
    /// 激活后的最初一瞬不算：那时 msctf 在把 profile 存的转换模式写回来，采纳它会让每个应用一激活
    /// 就是英文模式（表现：Ctrl+Space 好像「不能用」——其实只是每次都被打回英文）。
    pub(super) fn sync_from_conversion_mode(&self) {
        if let Some(until) = self.conversion_guard_until.get()
            && Instant::now() < until
        {
            log("激活后忽略一次系统写回的转换模式（msctf 的 profile 恢复，不是用户操作）");
            return;
        }
        let Some(thread_mgr) = self.thread_mgr.borrow().clone() else {
            return;
        };
        let Ok(compartment) = mode::conversion_compartment(&thread_mgr) else {
            return;
        };
        let english = mode::is_english(&compartment);
        if english != self.mode_state.english() {
            log(&format!(
                "转换模式变了（任务栏 / 系统快捷键），english={english}"
            ));
            self.set_english_mode(english);
        }
    }

    pub(super) fn add_lang_bar_item(&self) {
        let button = ModeButton::create(self.mode_state.clone());
        if let Some(thread_mgr) = self.thread_mgr.borrow().as_ref() {
            match thread_mgr.cast::<ITfLangBarItemMgr>() {
                Ok(mgr) => {
                    if let Err(error) = unsafe { mgr.AddItem(&button) } {
                        log(&format!("登记中英指示器失败: {error}"));
                    }
                }
                Err(error) => log(&format!("取语言栏管理器失败: {error}")),
            }
        }
        *self.mode_button.borrow_mut() = Some(button);
    }

    pub(super) fn remove_lang_bar_item(&self) {
        if let Some(button) = self.mode_button.borrow_mut().take()
            && let Some(thread_mgr) = self.thread_mgr.borrow().as_ref()
            && let Ok(mgr) = thread_mgr.cast::<ITfLangBarItemMgr>()
        {
            let _ = unsafe { mgr.RemoveItem(&button) };
        }
    }
}
