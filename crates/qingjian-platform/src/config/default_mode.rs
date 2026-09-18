use serde::{Deserialize, Serialize};

/// 新窗口（新线程第一次激活）用哪种输入模式。
///
/// 只管**首次激活那一次**：设完之后窗口内用户怎么切就怎么切（单击 Shift、点状态条都算），
/// 不会每次激活都把人打回默认 —— 那会在切应用时把正在打的中文顶掉。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DefaultMode {
    /// 不动：沿用系统（msctf 按 profile 恢复的）那一份，与改动前的行为一致。
    #[default]
    Last,

    /// 中文模式。
    Chinese,

    /// 英文模式。
    English,
}

impl DefaultMode {
    /// 全部取值，设置界面按这个顺序列出。
    pub const ALL: [Self; 3] = [Self::Last, Self::Chinese, Self::English];

    /// 配置文件里的写法。
    pub fn key(self) -> &'static str {
        match self {
            Self::Last => "last",
            Self::Chinese => "chinese",
            Self::English => "english",
        }
    }

    /// 界面上的名字。
    pub fn label(self) -> &'static str {
        match self {
            Self::Last => "记住上次",
            Self::Chinese => "中文",
            Self::English => "英文",
        }
    }

    /// 首次激活时要设成的模式（`true` 英文）；`None` = 不动。
    pub fn apply(self) -> Option<bool> {
        match self {
            Self::Last => None,
            Self::Chinese => Some(false),
            Self::English => Some(true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DefaultMode;

    /// 「记住上次」不动模式，另两个各给一个明确值 —— DLL 那边就靠这个返回值决定设不设。
    #[test]
    fn apply_maps_to_the_mode_to_set() {
        assert_eq!(DefaultMode::Last.apply(), None);
        assert_eq!(DefaultMode::Chinese.apply(), Some(false));
        assert_eq!(DefaultMode::English.apply(), Some(true));
    }

    /// 配置写法与界面名字一一对应（三个取值都要有，界面才列得全）。
    #[test]
    fn keys_and_labels_cover_all_values() {
        assert_eq!(DefaultMode::ALL.len(), 3);
        assert_eq!(DefaultMode::Last.key(), "last");
        assert_eq!(DefaultMode::Chinese.key(), "chinese");
        assert_eq!(DefaultMode::English.key(), "english");
        assert_eq!(DefaultMode::ALL[0].key(), "last");
    }
}
