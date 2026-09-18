//! 中 / 英模式：切模式先把组着的内容落定；指示器走语言栏按钮 + 转换模式 compartment，并推给 Server 的状态条；
//! 用户点任务栏中 / 英时由 compartment 回调反向同步。切换键与内置英文模式开关由 Server 经协议下发
//! （[`TextService_Impl::apply_input_settings`]），DLL 不读配置文件。

use std::time::{Duration, Instant};

use windows::Win32::UI::TextServices::ITfLangBarItemMgr;
use windows::core::Interface;

use qingjian_platform::protocol::InputSettings;

use super::TextService_Impl;
use crate::com::log::log;
use crate::com::mode::{self, ModeButton, conversion};

/// 激活后挡掉 msctf 写回 profile 的窗口。实测它约在激活后 90 ms 到，留足余量又不至于把用户
/// 手点任务栏中 / 英也挡掉（那个动作不会发生在激活后 0.5 秒内）。
const CONVERSION_RESTORE_WINDOW: Duration = Duration::from_millis(500);

impl TextService_Impl {
    /// 应用中英模式的设置：激活时与配置变更时都走这里。
    ///
    /// 任务栏那个图标画哪个字（`拼 / 双 / 五 / 注 / 中`，由 Server 按优先级挑一个下发，见 `ModeGlyph`）
    /// 变了要单独通知系统重取图标，否则要等到下次切模式才刷新。
    ///
    /// 内置英文模式开关**在运行中翻转**时，语言栏的中 / 英按钮与转换模式回调跟着登记 / 撤掉
    /// （激活时由 `Activate` 自己按开关登记，这里只管激活之后的变化），设置窗口改完不用切走再切回输入法。
    pub(super) fn apply_mode_settings(&self, input: &InputSettings) {
        let was_enabled = self.mode_state.enabled();
        let glyph_changed = self.mode_state.set_settings(
            input.english_mode,
            input.switch_mode,
            input.glyph,
            input.english_candidates,
        );
        // 关掉内置英文模式时立刻回中文，别停在一个再也切不回去的英文状态。
        if !input.english_mode && self.mode_state.english() {
            self.mode_state.set_english(false);
            self.refresh_mode_indicator();
        } else if glyph_changed {
            self.mode_state.notify();
        }
        if was_enabled != input.english_mode && self.is_active() {
            if input.english_mode {
                log("打开了内置英文模式：登记中 / 英按钮");
                self.add_lang_bar_item();
                self.advise_conversion_sink();
            } else {
                log("关掉了内置英文模式：撤掉中 / 英按钮");
                self.unadvise_conversion_sink();
                self.remove_lang_bar_item();
            }
        }
    }

    /// `Activate` 是否已经走完（[`super::ACTIVE`] 在它末尾才设）。
    fn is_active(&self) -> bool {
        super::ACTIVE.with(|active| active.borrow().is_some())
    }

    /// 应用 Server 下发的按键行为设置：`OpenSession` 的回包给一次，之后每一拍 `SyncMode` 也都带着。
    /// 值没变就什么都不做，所以设置窗口改完在下一拍（约 320 ms）生效，不用切走再切回输入法。
    pub(super) fn apply_input_settings(&self, input: InputSettings) {
        if self.input_settings.get() == Some(input) {
            return;
        }
        self.input_settings.set(Some(input));
        log(&format!(
            "按键行为设置：中英切换键 {}，内置英文模式 {}，新窗口默认模式 {}，注音模式 {}，Shift 字母进组句 {}，任务栏图标 {:?}，英文候选 {}",
            input.switch_mode.key(),
            input.english_mode,
            input.default_mode.key(),
            input.zhuyin,
            input.shift_letter_compose,
            input.glyph,
            input.english_candidates
        ));
        self.apply_mode_settings(&input);
    }

    /// 激活时把模式定到配置要的那一种，并把接下来一小段时间里 **msctf 写回 profile** 的那次变化挡掉。
    ///
    /// `[general] default_mode`：`chinese` / `english` 按它设；`last` = **不设**，让 msctf 把线程 profile
    /// 里记的「上次」写回来（这正是改动前实测的观感 —— 我们设的中文本来就被那次写回盖掉了）。
    /// **为什么要这段保护**：实测激活后约 90 ms msctf 会写回 profile，把刚设的值盖掉 —— 用户看到的就是
    /// 「设了默认中文，新开的记事本还是英文」。`conversion_guard_until` 就是给这一刻准备的，但 2026-09-18
    /// 之前它**只被读、没人写**（删 Ctrl+Space 那轮把写它的代码一起删了），所以一直是死保护。
    pub(super) fn start_mode_from_settings(&self) {
        let Some(input) = self.input_settings.get() else {
            log("激活时的初始模式：还没拿到 Server 的设置，不动（记住上次）");
            return;
        };
        let Some(english) = input.default_mode.apply() else {
            log("激活时的初始模式：记住上次（不动模式）");
            return;
        };
        log(&format!("激活时的初始模式：{}", input.default_mode.label()));
        self.mode_state.set_english(english);
        self.refresh_mode_indicator();
        self.conversion_guard_until
            .set(Some(Instant::now() + CONVERSION_RESTORE_WINDOW));
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

    /// 语言栏按钮换图标、写转换模式 compartment、把模式与 Caps 推给 Server（悬浮状态条）。
    pub(super) fn refresh_mode_indicator(&self) {
        let english = self.mode_state.english();
        // 状态条的模式格 Caps 亮着时前面加个「A」，所以要一起报（任务栏图标那格也看它）。
        let caps = crate::com::key::event::caps_lock_on();
        self.mode_state.notify();
        if let Some(thread_mgr) = self.thread_mgr.borrow().as_ref() {
            mode::set_indicator(thread_mgr, self.client_id.get(), english);
        }
        if let Some(client) = self.engine.borrow_mut().as_mut()
            && let Err(error) = client.mode_changed(english, caps)
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
