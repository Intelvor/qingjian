//! 「翻译选中文字」与「Ctrl+Space 切换中英」两个快捷键登记成 TSF **保留键**（preserved key）。
//! 带 Alt / Ctrl+Space 的组合是系统键，不经击键 sink（真机：Ctrl+Alt+T 在 `OnTestKeyDown` 里从没出现过）；
//! 保留键由 TSF 在应用之前匹配、回调 `OnPreservedKey`，UWP 里也一样。翻译组合来自
//! `[shortcut] translate_selection`，激活时读一次配置（AppContainer 读不到用户目录时用缺省 Ctrl+Alt+T）；
//! 切换键来自 `[shortcut] switch_mode`，那个值由 Server 经协议下发（DLL 不读配置文件），变了就地重登记。

use windows::Win32::UI::Input::KeyboardAndMouse::VK_SPACE;
use windows::Win32::UI::TextServices::{
    ITfKeystrokeMgr, TF_MOD_ALT, TF_MOD_CONTROL, TF_MOD_SHIFT, TF_PRESERVEDKEY,
};
use windows::core::{GUID, Result};

use qingjian_platform::protocol::{KeyEvent, KeyModifiers};
use qingjian_platform::{Config, KeyCombo};

use crate::com::log::log;

/// 本保留键的标识，`OnPreservedKey` 按它认。
pub(crate) const GUID_TRANSLATE: GUID = GUID::from_u128(0x5c0a7b12_3d4e_4f60_8a91_2b3c4d5e6f70);

/// Ctrl+Space 中英切换键的保留键标识。
pub(crate) const GUID_SWITCH_MODE: GUID = GUID::from_u128(0x2f6b8c51_9a34_4e7d_b2c8_5d1e0f3a7b64);

/// msctf.h 的 `TF_MOD_LWIN`（windows crate 没导出）。
const TF_MOD_LWIN: u32 = 0x08;

/// 读 `%APPDATA%\Qingjian\config.toml` 里的组合；读不到 / 解析失败用缺省。
pub(crate) fn load_combo() -> KeyCombo {
    let Some(path) = qingjian_platform::dirs::config_path() else {
        return KeyCombo::TRANSLATE_DEFAULT;
    };
    match Config::load(&path) {
        Ok(config) => config.shortcut.translate_selection,
        Err(error) => {
            log(&format!("读配置取翻译快捷键失败，用缺省: {error}"));
            KeyCombo::TRANSLATE_DEFAULT
        }
    }
}

/// Ctrl+Space 的 `TF_PRESERVEDKEY`。
fn switch_mode_key() -> TF_PRESERVEDKEY {
    TF_PRESERVEDKEY {
        uVKey: VK_SPACE.0 as u32,
        uModifiers: TF_MOD_CONTROL,
    }
}

/// 登记 Ctrl+Space 为中英切换保留键（`switch_mode = "ctrl+space"` 时）。
pub(crate) fn register_switch_mode(keystroke: &ITfKeystrokeMgr, tid: u32) -> Result<()> {
    let description: Vec<u16> = "切换中英文（青简）".encode_utf16().collect();
    unsafe { keystroke.PreserveKey(tid, &GUID_SWITCH_MODE, &switch_mode_key(), &description) }
}

pub(crate) fn unregister_switch_mode(keystroke: &ITfKeystrokeMgr) {
    let _ = unsafe { keystroke.UnpreserveKey(&GUID_SWITCH_MODE, &switch_mode_key()) };
}

/// imm.h 的 `IME_CHOTKEY_IME_NONIME_TOGGLE`（「输入法/非输入法切换」）在输入法热键表里的编号。
const HOTKEY_ID_TOGGLE_IME: &str = r"Control Panel\Input Method\Hot Keys\00000010";

/// Ctrl 在热键修饰键位里的那一位（imm.h 的 IME_HOTKEY_* 位；同一台机器上 `00000012` 是 Ctrl+.，
/// 与 Windows 的中英文标点切换默认值一致，据此确认这一位是 Ctrl）。
const HOTKEY_CTRL: u32 = 0x02;

/// 系统的「输入法/非输入法切换」是不是也绑在 **Ctrl+Space** 上（Windows 缺省如此）。
///
/// 绑着的话这个组合被系统先截走：我们的保留键收不到，而且系统那条路会把转换模式翻成「非原生」——
/// 我们又按 conversion compartment 把它当成英文模式，两边各切一次正好抵消，表现为「按了没反应」。
/// 所以这时不该再登记自己的保留键（见 `apply_mode_settings`）。读不到就按「没占用」处理，别误伤。
pub(crate) fn system_owns_ctrl_space() -> bool {
    let Ok(key) = windows_registry::CURRENT_USER.open(HOTKEY_ID_TOGGLE_IME) else {
        return false;
    };
    is_ctrl_space(raw_u32(&key, "Virtual Key"), raw_u32(&key, "Key Modifiers"))
}

/// 热键表里的两个值：虚拟键是 Space 且修饰键位里含 Ctrl。
fn is_ctrl_space(vk: Option<u32>, modifiers: Option<u32>) -> bool {
    let (Some(vk), Some(modifiers)) = (vk, modifiers) else {
        return false;
    };
    vk == VK_SPACE.0 as u32 && modifiers & HOTKEY_CTRL != 0
}

/// 热键表里的值是 4 字节小端 REG_BINARY。
fn raw_u32(key: &windows_registry::Key, name: &str) -> Option<u32> {
    let value = key.get_value(name).ok()?;
    let bytes: &[u8] = &value;
    (bytes.len() >= 4).then(|| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn preserved_key(combo: KeyCombo) -> TF_PRESERVEDKEY {
    let m = combo.modifiers;
    let mut modifiers = 0;
    if m.control {
        modifiers |= TF_MOD_CONTROL;
    }
    if m.option {
        modifiers |= TF_MOD_ALT;
    }
    if m.shift {
        modifiers |= TF_MOD_SHIFT;
    }
    if m.command {
        modifiers |= TF_MOD_LWIN;
    }
    TF_PRESERVEDKEY {
        uVKey: combo.key.to_ascii_uppercase() as u32,
        uModifiers: modifiers,
    }
}

pub(crate) fn register(keystroke: &ITfKeystrokeMgr, tid: u32, combo: KeyCombo) -> Result<()> {
    let key = preserved_key(combo);
    let description: Vec<u16> = "翻译选中文字".encode_utf16().collect();
    unsafe { keystroke.PreserveKey(tid, &GUID_TRANSLATE, &key, &description) }
}

pub(crate) fn unregister(keystroke: &ITfKeystrokeMgr, combo: KeyCombo) {
    let key = preserved_key(combo);
    let _ = unsafe { keystroke.UnpreserveKey(&GUID_TRANSLATE, &key) };
}

/// 保留键命中时喂给 Server 的按键：Router 按字符 + 物理修饰键与配置比对。
pub(crate) fn key_event(combo: KeyCombo, english_mode: bool) -> KeyEvent {
    let m = combo.modifiers;
    KeyEvent::new(
        combo.key.to_ascii_uppercase() as u32,
        Some(combo.key),
        KeyModifiers {
            ctrl: m.control,
            shift: m.shift,
            alt: m.option,
            win: m.command,
            caps: false,
            english_mode,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::is_ctrl_space;

    #[test]
    fn only_space_with_ctrl_counts_as_the_system_toggle() {
        // 真机上的值：`00000010` 虚拟键 0x20、修饰键 0xC002（低 4 位 0x2 = Ctrl）
        assert!(is_ctrl_space(Some(0x20), Some(0xC002)));
        assert!(!is_ctrl_space(Some(0x20), Some(0xC004))); // 只有别的修饰键
        assert!(!is_ctrl_space(Some(0xBE), Some(0xC002))); // `00000012` 是 Ctrl+.，不是切换键
        assert!(!is_ctrl_space(Some(0x20), Some(0))); // 未分配
        assert!(!is_ctrl_space(None, Some(0xC002))); // 读不到就按没占用处理
        assert!(!is_ctrl_space(Some(0x20), None));
    }
}
