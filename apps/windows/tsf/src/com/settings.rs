//! DLL 自己读的用户配置。
//!
//! DLL 跑在每个应用的进程里，拿不到 Server 那份 [`Config`]，但有两个值必须在按键到达**之前**就知道：
//! 中英切换键（单击判定在 `OnTestKeyUp` 里做）与内置英文模式开关（决定要不要登记语言栏按钮）。
//! 激活时读一次，之后靠 [`modified`] 盯 mtime 热加载——设置窗口改完不用切走再切回输入法（见
//! [`TextService_Impl::reload_settings_if_changed`](crate::com::service::TextService_Impl::reload_settings_if_changed)）。

use std::path::PathBuf;
use std::time::SystemTime;

use qingjian_platform::Config;

/// 读用户配置；`APPDATA` 取不到（AppContainer 里的应用）或文件读不了 / 解析失败时用缺省。
pub(crate) fn load() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };
    Config::load(&path).unwrap_or_else(|error| {
        crate::com::log::log(&format!("读配置失败，用缺省: {error}"));
        Config::default()
    })
}

/// 配置文件的修改时间，用来判断要不要重读；文件不在或读不到元数据时为 `None`。
pub(crate) fn modified() -> Option<SystemTime> {
    std::fs::metadata(config_path()?).ok()?.modified().ok()
}

/// `%APPDATA%\Qingjian\config.toml`；Server 与设置程序也认这一份。
pub(crate) fn config_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|base| PathBuf::from(base).join("Qingjian").join("config.toml"))
}
