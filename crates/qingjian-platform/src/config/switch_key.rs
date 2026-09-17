use serde::{Deserialize, Serialize};

/// 中 / 英模式切换键（Windows）。`shift` / `control` 是**单击**那个修饰键；`none` 不切。
/// macOS 的切换键是 Caps Lock，本项不生效。
///
/// 曾经还有过 `ctrl+space`（走 TSF 保留键登记），已移除：那个组合与系统的「输入法/非输入法切换」
/// 抢得厉害，体验不成熟。旧配置里如果写着它，解析会失败、整体退回缺省的单击 Shift。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SwitchKey {
    /// 单击 Shift（缺省）。与微软拼音一致，但打字时容易误触。
    #[default]
    Shift,

    /// 单击 Ctrl：Shift 老是误触时换它。
    #[serde(alias = "ctrl")]
    Control,

    /// 不切换：只剩语言栏 / 悬浮状态条上的按钮能切。
    #[serde(alias = "off", alias = "disabled")]
    None,
}

impl SwitchKey {
    /// 全部取值，设置界面按这个顺序列出。
    pub const ALL: [Self; 3] = [Self::Shift, Self::Control, Self::None];

    /// 配置文件里的写法。
    pub const fn key(self) -> &'static str {
        match self {
            Self::Shift => "shift",
            Self::Control => "control",
            Self::None => "none",
        }
    }

    /// 界面上的名字。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Shift => "单击 Shift",
            Self::Control => "单击 Ctrl",
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
        assert_eq!(parse("none").unwrap(), SwitchKey::None);
        assert_eq!(parse("off").unwrap(), SwitchKey::None);
        assert_eq!(SwitchKey::Control.key(), "control");
        assert_eq!(SwitchKey::default(), SwitchKey::Shift);
    }

    #[test]
    fn unknown_values_are_rejected() {
        assert!(parse("hyper").is_err());
        // Ctrl+Space 已移除：旧配置整体解析失败，Server 会退回缺省（见 Server 侧的 Config::load）
        assert!(parse("ctrl+space").is_err());
    }
}
