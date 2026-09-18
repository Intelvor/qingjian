//! `ITfTextInputProcessor`：激活时挂击键 sink、登记翻译保留键、连 Server、起轮询定时器、挂 profile /
//! 转换模式回调、登记语言栏按钮；停用按相反顺序撤掉，敲了一半的拼音先原样落定。

use windows::Win32::UI::TextServices::{
    ITfKeyEventSink, ITfKeystrokeMgr, ITfTextInputProcessor_Impl, ITfThreadMgr,
};
use windows::core::{IUnknownImpl, Interface, Ref, Result};

use qingjian_platform::protocol::InputSettings;

use super::{ACTIVE, TextService_Impl};
use crate::com::key::preserved;
use crate::com::log::log;
use crate::com::poll::PollTimer;
use crate::com::profile;

impl ITfTextInputProcessor_Impl for TextService_Impl {
    fn Activate(&self, ptim: Ref<ITfThreadMgr>, tid: u32) -> Result<()> {
        let thread_mgr = ptim.ok()?.clone();
        let keystroke: ITfKeystrokeMgr = thread_mgr.cast()?;
        let sink: ITfKeyEventSink = self.to_interface();
        // client id 必须在挂击键 sink **之前**记下：`AdviseKeyEventSink` 最后那个参数是「本线程在前台」，
        // msctf 会当场回调一次 `OnSetFocus`，那里就要连 Server 并刷指示器（写转换模式 compartment 用的
        // 就是这个 client id）。记晚了那次写入会带着空 client id 失败，任务栏的中 / 英就刷不出来。
        self.client_id.set(tid);
        unsafe { keystroke.AdviseKeyEventSink(tid, &sink, true)? };
        let combo = preserved::load_combo();
        match preserved::register(&keystroke, tid, combo) {
            Ok(()) => {
                self.translate_combo.set(Some(combo));
                log(&format!("翻译选中文字快捷键已登记为保留键: {combo}"));
            }
            Err(error) => log(&format!("登记翻译快捷键失败: {error}")),
        }

        // 连不上 Server、没定时器都不致命。
        match PollTimer::new(self.engine.clone(), self.shared.clone()) {
            Ok(timer) => *self.poll_timer.borrow_mut() = Some(timer),
            Err(error) => log(&format!("挂云联想轮询定时器失败: {error}")),
        }

        if self.profile_cookie.get().is_none() {
            match profile::advise(&thread_mgr, super::session_id()) {
                Ok(cookie) => self.profile_cookie.set(Some(cookie)),
                Err(error) => log(&format!("监听输入法切换失败: {error}")),
            }
        }
        *self.thread_mgr.borrow_mut() = Some(thread_mgr);
        // 连 Server：它随 `OpenSession` 的回包把按键行为设置带下来，就地应用（那两个值在按键到达之前就要有）。
        self.connect();
        // 连不上 Server 时用缺省值把模式状态建起来；连上了的话上面已应用过真实值，这里去重跳过。
        self.apply_input_settings(InputSettings::default());
        // 激活时的初始模式：按 `[general] default_mode` 定，并挡掉 msctf 随后写回 profile 的那次变化。
        self.start_mode_from_settings();
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
        log("青简 TSF 已停用");
        Ok(())
    }
}
