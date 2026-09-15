//! `ITfTextInputProcessor`：激活时挂击键 sink、登记翻译保留键、连 Server、起轮询定时器、挂 profile /
//! 转换模式回调、登记语言栏按钮；停用按相反顺序撤掉，敲了一半的拼音先原样落定。

use std::time::Instant;

use windows::Win32::UI::TextServices::{
    ITfKeyEventSink, ITfKeystrokeMgr, ITfTextInputProcessor_Impl, ITfThreadMgr,
};
use windows::core::{IUnknownImpl, Interface, Ref, Result};

use qingjian_platform::protocol::SessionId;

use super::mode::CONVERSION_RESTORE_GUARD;
use super::{ACTIVE, TextService_Impl};
use crate::com::key::preserved;
use crate::com::log::log;
use crate::com::poll::PollTimer;
use crate::com::profile;
use crate::com::settings;

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, ptim: Ref<ITfThreadMgr>, tid: u32) -> Result<()> {
        let thread_mgr = ptim.ok()?.clone();
        let keystroke: ITfKeystrokeMgr = thread_mgr.cast()?;
        let sink: ITfKeyEventSink = self.to_interface();
        unsafe { keystroke.AdviseKeyEventSink(tid, &sink, true)? };
        let combo = preserved::load_combo();
        match preserved::register(&keystroke, tid, combo) {
            Ok(()) => {
                self.translate_combo.set(Some(combo));
                log(&format!("翻译选中文字快捷键已登记为保留键: {combo}"));
            }
            Err(error) => log(&format!("登记翻译快捷键失败: {error}")),
        }

        self.client_id.set(tid);
        // 连不上 Server、没定时器都不致命。
        self.connect();
        match PollTimer::new(self.engine.clone(), self.shared.clone()) {
            Ok(timer) => *self.poll_timer.borrow_mut() = Some(timer),
            Err(error) => log(&format!("挂云联想轮询定时器失败: {error}")),
        }

        if self.profile_cookie.get().is_none() {
            match profile::advise(&thread_mgr, SessionId(tid as u64)) {
                Ok(cookie) => self.profile_cookie.set(Some(cookie)),
                Err(error) => log(&format!("监听输入法切换失败: {error}")),
            }
        }
        *self.thread_mgr.borrow_mut() = Some(thread_mgr);
        // 中英模式的两个设置（切换键、内置英文模式开关）在按键到达之前就要有：激活时读一次，
        // 之后由轮询按 mtime 热加载（`reload_settings_if_changed`），设置窗口改完不用切走再切回。
        let config = settings::load();
        self.config_stamp.set(settings::modified());
        self.apply_mode_settings(config.general.english_mode, config.shortcut.switch_mode);
        // 一小段时间内不理系统写回的转换模式（见 `sync_from_conversion_mode`），
        // 否则 msctf 会在激活后把 profile 存的「非原生」写回来，每个应用一激活就是英文模式。
        self.conversion_guard_until
            .set(Some(Instant::now() + CONVERSION_RESTORE_GUARD));
        log(&format!(
            "中英切换键 {}，内置英文模式 {}",
            config.shortcut.switch_mode.key(),
            config.general.english_mode
        ));
        self.mode_state.set_english(false);
        self.refresh_mode_indicator();
        if self.mode_state.enabled() {
            self.add_lang_bar_item();
            // 放在初始写指示器之后，别被自己那次写触发。
            self.advise_conversion_sink();
        } else {
            log("配置关掉了内置英文模式：不登记中 / 英按钮，固定中文模式");
        }
        ACTIVE.with(|active| *active.borrow_mut() = Some(self.to_object()));
        log(&format!("青简 TSF 已激活 tid={tid}"));
        Ok(())
    }

    fn Deactivate(&self) -> Result<()> {
        // 先撤回调，之后不再有回调碰本服务。
        ACTIVE.with(|active| active.borrow_mut().take());
        self.unadvise_conversion_sink();
        self.remove_lang_bar_item();
        self.poll_timer.borrow_mut().take();
        // 切走输入法时敲了一半的拼音原样落定，再关会话。
        self.commit_pending();
        if let Some(thread_mgr) = self.thread_mgr.borrow_mut().take()
            && let Ok(keystroke) = thread_mgr.cast::<ITfKeystrokeMgr>()
        {
            self.drop_switch_preserved_key(&keystroke);
            if let Some(combo) = self.translate_combo.take() {
                preserved::unregister(&keystroke, combo);
            }
            let _ = unsafe { keystroke.UnadviseKeyEventSink(self.client_id.get()) };
        }
        if let Some(client) = self.engine.borrow_mut().take() {
            let _ = client.close();
        }
        self.shared.reset();
        self.shared.take_server_stale();
        self.shared.set_foreground(false);
        log("青简 TSF 已停用");
        Ok(())
    }
}
