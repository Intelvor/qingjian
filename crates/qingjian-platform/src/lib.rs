//! 平台层共用的、与具体窗口系统无关的部分：配置文件，以及将来 Core 与壳之间的协议类型。
//!
//! 这里的类型必须可序列化：macOS / Linux 上 Core 与壳同进程，Windows 上 Core 在独立
//! Server 进程，同一套类型两边都用。

mod config;
pub mod dirs;
mod error;
pub mod extra_dictionaries;
pub mod logs;
pub mod protocol;
pub mod resources;

pub use config::{
    AccentColor, AppsConfig, CandidateRenderer, Config, DEFAULT_DOMAINS,
    DEFAULT_ENGLISH_CANDIDATES_OFF, DEFAULT_ENGLISH_CANDIDATES_OFF_LINUX,
    DEFAULT_ENGLISH_CANDIDATES_OFF_MACOS, DEFAULT_ENGLISH_CANDIDATES_OFF_WINDOWS,
    DEFAULT_PAGE_KEYS, DefaultMode, DictionariesConfig, GeneralConfig, KeyCombo,
    LEARNING_LANGUAGE_OFF, LayoutMode, LocalModelConfig, LogLevel, MAX_PAGE_SIZE, Modifiers,
    PAGE_KEY_OPTIONS, PreeditMode, Scheme, ShiftLetter, ShortcutConfig, SwitchKey, ThemeMode,
    scheme_label,
};
pub use error::ConfigError;

// 云联想上下文长度的上限：TSF 侧要按它读光标前后的文本，从这儿转出去省得抄一遍。
pub use qingjian_predict::{MAX_LOOKAHEAD, MAX_LOOKBACK};
