use serde::{Deserialize, Serialize};

/// 中 / 英模式切换键（Windows）。`shift` / `control` 是**单击**那个修饰键；`ctrl+space` 是组合键
/// （走 TSF 保留键登记，与「翻译选中文字」同一套机制）；`none` 不切。macOS 的切换键是 Caps Lock，本项不生效。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SwitchKey {
    /// 单击 Shift（缺省）。与微软拼音一致，但打字时容易误触。
    #[default]
    Shift,

    /// 单击 Ctrl：Shift 老是误触时换它。
    #[serde(alias = "ctrl")]
    Control,

    /// Ctrl + Space 组合键。系统若把「输入法/非输入法切换」也绑在它上面会抢先，需要先关掉那个系统热键。
    #[serde(rename = "ctrl+space", alias = "control+space")]
    CtrlSpace,

    /// 不切换：只剩语言栏 / 悬浮状态条上的按钮能切。
    #[serde(alias = "off", alias = "disabled")]
    None,
}

impl SwitchKey {
    /// 全部取值，设置界面按这个顺序列出。
    pub const ALL: [Self; 4] = [Self::Shift, Self::Control, Self::CtrlSpace, Self::None];

    /// 配置文件里的写法。
    pub const fn key(self) -> &'static str {
        match self {
            Self::Shift => "shift",
            Self::Control => "control",
            Self::CtrlSpace => "ctrl+space",
            Self::None => "none",
        }
    }

    /// 界面上的名字。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Shift => "单击 Shift",
            Self::Control => "单击 Ctrl",
            Self::CtrlSpace => "Ctrl + Space",
            Self::None => "不切换",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Wrapper {
        k: SwitchKey,
    }

    fn parse(text: &str) -> Result<SwitchKey, toml::de::Error> {
        toml::from_str::<Wrapper>(&format!("k = \"{text}\"")).map(|wrapper| wrapper.k)
    }

    #[test]
    fn parses_aliases_and_prints_canonically() {
        assert_eq!(parse("shift").unwrap(), SwitchKey::Shift);
        assert_eq!(parse("control").unwrap(), SwitchKey::Control);
        assert_eq!(parse("ctrl").unwrap(), SwitchKey::Control);
        assert_eq!(parse("ctrl+space").unwrap(), SwitchKey::CtrlSpace);
        assert_eq!(parse("control+space").unwrap(), SwitchKey::CtrlSpace);
        assert_eq!(parse("none").unwrap(), SwitchKey::None);
        assert_eq!(parse("off").unwrap(), SwitchKey::None);
        assert_eq!(SwitchKey::Control.key(), "control");
        assert_eq!(SwitchKey::CtrlSpace.key(), "ctrl+space");
        assert_eq!(SwitchKey::default(), SwitchKey::Shift);
    }

    #[test]
    fn unknown_values_are_rejected() {
        assert!(parse("hyper").is_err());
    }
}
