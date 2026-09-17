use serde::{Deserialize, Serialize};

/// 主题色（整体强调色）。
///
/// 管两处：候选窗的**高亮 / 悬停底色**，以及状态条上强调格（「中 / 英」「，。」开着时）与云朵图标的**品牌色**。
/// 语义色不吃它 —— 生词译文的橙、译文 / 序号的灰都照旧。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccentColor {
    /// 青简绿。
    #[default]
    Qingjian,

    /// 经典蓝。
    Classic,
}

impl AccentColor {
    /// 全部取值，设置界面按这个顺序列出。
    pub const ALL: [Self; 2] = [Self::Qingjian, Self::Classic];

    /// 配置文件里的写法。
    pub fn key(self) -> &'static str {
        match self {
            Self::Qingjian => "qingjian",
            Self::Classic => "classic",
        }
    }

    /// 界面上的名字。
    pub fn label(self) -> &'static str {
        match self {
            Self::Qingjian => "青简绿",
            Self::Classic => "经典蓝",
        }
    }
}
