//! 单击中 / 英切换键的判定，喂的是击键 sink 的 `OnTestKeyDown` / `OnTestKeyUp`（被吃掉的键也经过它们，
//! 与 `WH_KEYBOARD` 钩子不同）。按下切换键到抬起之间没插进别的键，就是一次单击。
//!
//! 切换键来自 `[shortcut] switch_mode`：`shift`（缺省）/ `control` / `none`（不切换，见 issue #81）。

use std::cell::Cell;

use windows::Win32::Foundation::LPARAM;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_CONTROL, VK_LCONTROL, VK_LSHIFT, VK_RCONTROL, VK_RSHIFT, VK_SHIFT,
};

use qingjian_platform::SwitchKey;

#[derive(Default)]
pub(crate) struct KeyTap {
    /// 按下切换键后还没有别的键插进来。
    alone: Cell<bool>,
}

impl KeyTap {
    /// 任一键按下。`lparam` 第 30 位是按下前的状态（1 = 自动重复，不算新按下）。
    pub(crate) fn key_down(&self, vk: u32, lparam: LPARAM, key: SwitchKey) {
        if !matches_key(key, vk) {
            self.alone.set(false);
        } else if (lparam.0 >> 30) & 1 == 0 {
            self.alone.set(true);
        }
    }

    /// 任一键抬起；切换键单独抬起返回 `true`，一次抬起只算一次。
    pub(crate) fn key_up(&self, vk: u32, key: SwitchKey) -> bool {
        matches_key(key, vk) && self.alone.replace(false)
    }
}

/// 这个虚拟键码是不是切换键（左右两个都算）。`none` 谁都不算；`ctrl+space` 是组合键，由保留键处理，不走单击判定。
fn matches_key(key: SwitchKey, vk: u32) -> bool {
    match key {
        SwitchKey::Shift => {
            vk == VK_SHIFT.0 as u32 || vk == VK_LSHIFT.0 as u32 || vk == VK_RSHIFT.0 as u32
        }
        SwitchKey::Control => {
            vk == VK_CONTROL.0 as u32 || vk == VK_LCONTROL.0 as u32 || vk == VK_RCONTROL.0 as u32
        }
        SwitchKey::CtrlSpace | SwitchKey::None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 按下再抬起的 lparam（第 30 位为 0 表示新按下）。
    const DOWN: LPARAM = LPARAM(0);
    const VK_SHIFT_LEFT: u32 = 0xA0;
    const VK_CONTROL_LEFT: u32 = 0xA2;

    #[test]
    fn shift_tap_fires_only_when_nothing_else_interrupts() {
        let tap = KeyTap::default();
        tap.key_down(VK_SHIFT_LEFT, DOWN, SwitchKey::Shift);
        assert!(tap.key_up(VK_SHIFT_LEFT, SwitchKey::Shift));
        // 一次抬起只算一次
        assert!(!tap.key_up(VK_SHIFT_LEFT, SwitchKey::Shift));

        tap.key_down(VK_SHIFT_LEFT, DOWN, SwitchKey::Shift);
        tap.key_down(0x41, DOWN, SwitchKey::Shift); // 中间插了一个 A
        assert!(!tap.key_up(VK_SHIFT_LEFT, SwitchKey::Shift));
    }

    #[test]
    fn control_is_the_switch_key_when_configured() {
        let tap = KeyTap::default();
        // 配成 control 后 Shift 不再算切换键
        tap.key_down(VK_SHIFT_LEFT, DOWN, SwitchKey::Control);
        assert!(!tap.key_up(VK_SHIFT_LEFT, SwitchKey::Control));

        tap.key_down(VK_CONTROL_LEFT, DOWN, SwitchKey::Control);
        assert!(tap.key_up(VK_CONTROL_LEFT, SwitchKey::Control));
    }

    #[test]
    fn none_and_combo_never_fire_as_a_tap() {
        let tap = KeyTap::default();
        // 「不切换」与「Ctrl+Space」（组合键，走保留键）都不是单击某个修饰键
        for key in [SwitchKey::None, SwitchKey::CtrlSpace] {
            tap.key_down(VK_SHIFT_LEFT, DOWN, key);
            assert!(!tap.key_up(VK_SHIFT_LEFT, key));
            tap.key_down(VK_CONTROL_LEFT, DOWN, key);
            assert!(!tap.key_up(VK_CONTROL_LEFT, key));
        }
    }

    #[test]
    fn auto_repeat_does_not_rearm() {
        let tap = KeyTap::default();
        // 第 30 位为 1：自动重复，不算新按下
        let repeat = LPARAM(1 << 30);
        tap.key_down(VK_SHIFT_LEFT, DOWN, SwitchKey::Shift);
        assert!(tap.key_up(VK_SHIFT_LEFT, SwitchKey::Shift));
        tap.key_down(VK_SHIFT_LEFT, repeat, SwitchKey::Shift);
        assert!(!tap.key_up(VK_SHIFT_LEFT, SwitchKey::Shift));
    }
}
