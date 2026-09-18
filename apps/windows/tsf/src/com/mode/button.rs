//! 中 / 英 / 注输入模式指示器：Win11 托盘品牌图标左边的模式图标。按微软 IME 的做法经 `GUID_LBI_INPUTMODE`
//! 语言栏按钮把图标交给系统（转换模式 compartment 不走这条通道，光写它不显示）。Caps Lock 亮着显示「A」。

use std::rc::Rc;

use windows::Win32::Foundation::{E_NOINTERFACE, POINT, RECT};
use windows::Win32::UI::TextServices::{
    GUID_LBI_INPUTMODE, ITfLangBarItem_Impl, ITfLangBarItemButton, ITfLangBarItemButton_Impl,
    ITfLangBarItemSink, ITfMenu, ITfSource, ITfSource_Impl, TF_LANGBARITEMINFO,
    TF_LBI_STYLE_BTN_BUTTON, TfLBIClick,
};
use windows::Win32::UI::WindowsAndMessaging::HICON;
use windows::core::{BOOL, BSTR, GUID, IUnknown, Interface, Ref, Result, implement};

use qingjian_platform::SwitchKey;

use super::ModeState;
use super::icon::{self, Glyph};
use crate::com::CLSID_QINGJIAN;
use crate::com::key::event::caps_lock_on;

/// `GUID_LBI_INPUTMODE` 语言栏按钮：图标随 [`ModeState`] 显示中 / 英，点它切模式。
#[implement(ITfLangBarItemButton, ITfSource)]
pub(crate) struct ModeButton {
    state: Rc<ModeState>,
}

impl ModeButton {
    pub(crate) fn create(state: Rc<ModeState>) -> ITfLangBarItemButton {
        Self { state }.into()
    }
}

impl ITfLangBarItem_Impl for ModeButton_Impl {
    fn GetInfo(&self, pinfo: *mut TF_LANGBARITEMINFO) -> Result<()> {
        let info = unsafe { &mut *pinfo };
        info.clsidService = CLSID_QINGJIAN;
        info.guidItem = GUID_LBI_INPUTMODE;
        info.dwStyle = TF_LBI_STYLE_BTN_BUTTON;
        info.ulSort = 0;
        let desc: Vec<u16> = "青简中英模式".encode_utf16().collect();
        let n = desc.len().min(info.szDescription.len());
        info.szDescription[..n].copy_from_slice(&desc[..n]);
        Ok(())
    }

    fn GetStatus(&self) -> Result<u32> {
        Ok(0)
    }

    fn Show(&self, _fshow: BOOL) -> Result<()> {
        Ok(())
    }

    fn GetTooltipString(&self) -> Result<BSTR> {
        let text = if !self.state.enabled() {
            "中 / 英（内置英文模式已关闭）"
        } else {
            match self.state.switch_key() {
                SwitchKey::Shift => "中 / 英（单击 Shift 切换）",
                SwitchKey::Control => "中 / 英（单击 Ctrl 切换）",
                SwitchKey::None => "中 / 英（未设切换键，点这里切换）",
            }
        };
        Ok(BSTR::from(text))
    }
}

impl ITfLangBarItemButton_Impl for ModeButton_Impl {
    fn OnClick(&self, _click: TfLBIClick, _pt: &POINT, _prcarea: *const RECT) -> Result<()> {
        crate::com::service::toggle_mode();
        Ok(())
    }

    fn InitMenu(&self, _pmenu: Ref<ITfMenu>) -> Result<()> {
        Ok(())
    }

    fn OnMenuSelect(&self, _wid: u32) -> Result<()> {
        Ok(())
    }

    fn GetIcon(&self) -> Result<HICON> {
        icon::make(self.glyph())
    }

    fn GetText(&self) -> Result<BSTR> {
        Ok(BSTR::from(match self.glyph() {
            Glyph::Chinese => "中",
            Glyph::English => "英",
            Glyph::Zhuyin => "注",
            Glyph::CapsLock => "A",
        }))
    }
}

impl ModeButton_Impl {
    /// Caps 亮着无论中英模式都直接出大写英文，所以它优先。
    fn glyph(&self) -> Glyph {
        glyph_for(caps_lock_on(), self.state.english(), self.state.zhuyin())
    }
}

/// 图标选哪个：Caps > 英 > 注 > 中，与状态条模式格的文字同一套优先级
/// （状态条把 Caps 并进文字写成「A 中」，任务栏一格放不下两个字，Caps 亮着就只出「A」）。
fn glyph_for(caps: bool, english: bool, zhuyin: bool) -> Glyph {
    if caps {
        Glyph::CapsLock
    } else if english {
        Glyph::English
    } else if zhuyin {
        Glyph::Zhuyin
    } else {
        Glyph::Chinese
    }
}

impl ITfSource_Impl for ModeButton_Impl {
    fn AdviseSink(&self, riid: *const GUID, punk: Ref<IUnknown>) -> Result<u32> {
        if unsafe { *riid } != ITfLangBarItemSink::IID {
            return Err(E_NOINTERFACE.into());
        }
        let sink: ITfLangBarItemSink = punk.ok()?.cast()?;
        *self.state.sink.borrow_mut() = Some(sink);
        Ok(1) // 只支持一个回调，cookie 固定
    }

    fn UnadviseSink(&self, _dwcookie: u32) -> Result<()> {
        *self.state.sink.borrow_mut() = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 优先级：Caps 亮着出「A」，其次英文、注音、中文。
    #[test]
    fn glyph_priority_is_caps_then_english_then_zhuyin() {
        assert_eq!(glyph_for(true, true, true), Glyph::CapsLock);
        assert_eq!(glyph_for(true, false, true), Glyph::CapsLock);
        assert_eq!(glyph_for(false, true, true), Glyph::English);
        assert_eq!(glyph_for(false, false, true), Glyph::Zhuyin);
        assert_eq!(glyph_for(false, false, false), Glyph::Chinese);
    }
}
