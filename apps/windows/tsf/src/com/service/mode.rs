//! 中 / 英模式：切模式先把组着的内容落定；指示器走语言栏按钮 + 转换模式 compartment，并推给 Server 的状态条；
//! 用户点任务栏中 / 英时由 compartment 回调反向同步。切换键与内置英文模式开关来自配置，激活时读一次、
//! 之后按 mtime 热加载（[`TextService_Impl::reload_settings_if_changed`]）。

use std::time::Instant;

use windows::Win32::UI::TextServices::{ITfKeystrokeMgr, ITfLangBarItemMgr};
use windows::core::Interface;

use qingjian_platform::SwitchKey;

use super::TextService_Impl;
use crate::com::key::preserved;
use crate::com::log::log;
use crate::com::mode::{self, ModeButton, conversion};
use crate::com::settings;

/// 激活后多久内忽略系统写回的转换模式：msctf 在 TIP 激活后约 200–300 ms 会把 profile 存的
/// 转换模式（缺省「非原生」= 关掉输入法）写回 compartment，不忽略的话每个应用一激活就被翻成英文模式。
pub(super) const CONVERSION_RESTORE_GUARD: std::time::Duration =
    std::time::Duration::from_millis(1200);

impl TextService_Impl {
    /// 应用中英模式的两项设置：激活时与配置变更时都走这里。
    pub(super) fn apply_mode_settings(&self, english_mode: bool, switch_key: SwitchKey) {
        self.mode_state.set_settings(english_mode, switch_key);
        // 关掉内置英文模式时立刻回中文，别停在一个再也切不回去的英文状态。
        if !english_mode && self.mode_state.english() {
            self.mode_state.set_english(false);
            self.refresh_mode_indicator();
        }
        self.sync_switch_preserved_key(switch_key);
    }

    /// 配置文件改了就地重读（mtime 变了才算）：设置窗口改完立刻生效，不用切走再切回输入法。
    pub(super) fn reload_settings_if_changed(&self) {
        let Some(stamp) = settings::modified() else {
            return;
        };
        if self.config_stamp.get() == Some(stamp) {
            return;
        }
        self.config_stamp.set(Some(stamp));
        let config = settings::load();
        log(&format!(
            "配置已变更：中英切换键 {}，内置英文模式 {}",
            config.shortcut.switch_mode.key(),
            config.general.english_mode
        ));
        self.apply_mode_settings(config.general.english_mode, config.shortcut.switch_mode);
    }

    /// Ctrl+Space 是组合键、走 TSF 保留键（与「翻译选中文字」同一套）；换成别的键就撤掉登记，
    /// 免得白占住 Ctrl+Space。
    ///
    /// 但 Windows 缺省把「输入法/非输入法切换」也绑在 Ctrl+Space 上：那时系统先截走这个组合，
    /// 我们的保留键根本收不到，而系统那条路会把转换模式翻成「非原生」、我们再按 conversion compartment
    /// 同步成英文模式——两边各切一次正好抵消。这种情况下不登记自己的键，交给系统那条路。
    fn sync_switch_preserved_key(&self, switch_key: SwitchKey) {
        let configured = matches!(switch_key, SwitchKey::CtrlSpace);
        let system_owns_it = configured && preserved::system_owns_ctrl_space();
        let want = configured && !system_owns_it;
        if want == self.switch_preserved.get() {
            if system_owns_it {
                log(
                    "系统的「输入法/非输入法切换」占着 Ctrl+Space：切中英交给它（不再重复登记保留键）",
                );
            }
            return;
        }
        let Some(thread_mgr) = self.thread_mgr.borrow().clone() else {
            return;
        };
        let Ok(keystroke) = thread_mgr.cast::<ITfKeystrokeMgr>() else {
            return;
        };
        if want {
            match preserved::register_switch_mode(&keystroke, self.client_id.get()) {
                Ok(()) => {
                    self.switch_preserved.set(true);
                    log("中英切换键 Ctrl+Space 已登记为保留键");
                }
                Err(error) => log(&format!("登记 Ctrl+Space 切换键失败: {error}")),
            }
        } else {
            preserved::unregister_switch_mode(&keystroke);
            self.switch_preserved.set(false);
            if system_owns_it {
                log(
                    "系统的「输入法/非输入法切换」占着 Ctrl+Space：切中英交给它（已撤掉自己的保留键）",
                );
            }
        }
    }

    /// 停用时撤掉 Ctrl+Space 的保留键登记。
    pub(super) fn drop_switch_preserved_key(&self, keystroke: &ITfKeystrokeMgr) {
        if self.switch_preserved.replace(false) {
            preserved::unregister_switch_mode(keystroke);
        }
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
