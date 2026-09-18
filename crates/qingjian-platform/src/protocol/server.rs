use serde::{Deserialize, Serialize};

use super::frame::Frame;
use super::key::KeyOutcome;
use super::session::SessionId;
use crate::config::{DefaultMode, Scheme, SwitchKey};

/// 任务栏那张模式图标画哪个字（Caps 的「A」与英文模式的「英」由 DLL 自己定，不看这一项）。
///
/// DLL 端只有**一格 16px 位图**，混输（`拼五`）放不下两个字，所以由 Server 按优先级挑一个发下来：
/// 五笔 > 注音 > 双拼 > 全拼 —— 五笔开着时出「五」更能说明形码这一轴是活的。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModeGlyph {
    /// 全拼。
    #[default]
    Pinyin,

    /// 双拼（哪一套不影响这张图）。
    Shuangpin,

    /// 大千注音。
    Zhuyin,

    /// 五笔（与拼音同时开着时也出它）。
    Wubi,

    /// 两条轴都关着（配置写成了 `scheme = "none"` 又没开五笔）：与状态条一样退回「中」。
    Chinese,
}

impl ModeGlyph {
    /// 按 `[general] scheme` 与 `[general] wubi` 挑一个字。
    pub fn for_scheme(scheme: Scheme, wubi: bool) -> Self {
        if wubi {
            return Self::Wubi;
        }
        match scheme {
            Scheme::Pinyin => Self::Pinyin,
            Scheme::Shuangpin(_) => Self::Shuangpin,
            Scheme::Zhuyin => Self::Zhuyin,
            Scheme::Off => Self::Chinese,
        }
    }
}

/// Server 下发给 DLL 的「按键行为」设置。
///
/// DLL 跑在每个应用的进程里，拿不到 Server 那份 [`Config`](crate::Config)，但这两个值在**按键到达之前**
/// 就得知道：单击切换键的判定在 `OnTestKeyUp` 里做，内置英文模式开关决定要不要登记语言栏按钮。
/// 所以由 Server 读配置（它本来就在盯热加载）经协议下发，DLL 不读文件、不查 mtime——
/// `%APPDATA%\Qingjian` 对 AppContainer 里的商店应用本来也读不到。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputSettings {
    /// 中英切换键（`[shortcut] switch_mode`）。
    pub switch_mode: SwitchKey,

    /// 内置英文模式总开关（`[general] english_mode`）。
    pub english_mode: bool,

    /// 新窗口（新线程第一次激活）要设成的模式（`[general] default_mode`）；`last` = 不动。
    /// **加字段向后兼容**：老 DLL 忽略它，新 DLL 对老 Server 拿到的缺省也是「不动」。
    #[serde(default)]
    pub default_mode: DefaultMode,

    /// 注音模式（`[general] zhuyin`）：任务栏模式图标据此出「注」（悬浮状态条那格也看它）。
    /// **加字段向后兼容**：老 DLL 忽略它，新 DLL 对老 Server 拿到的缺省是关。
    #[serde(default)]
    pub zhuyin: bool,

    /// 中文模式下 Shift+字母进组句（`[general] shift_letter = "compose"`）。DLL 据此决定没在组句时
    /// 按住 Shift 敲的字母吃不吃：缺省交给应用，开着时送 Server 起一段组句（`⇧C` 接 `pan` 出「C盘」）。
    #[serde(default)]
    pub shift_letter_compose: bool,

    /// 任务栏模式图标画哪个字（`拼` / `双` / `注` / `五` / `中`）：任务栏只有一格位图，放不下两个字母，
    /// 所以由 Server 按优先级挑一个（见 [`ModeGlyph::for_scheme`]）。Caps 亮着时 DLL 出「A」、英文模式出
    /// 「英」，都不看这一项。**加字段向后兼容**：老 DLL 忽略它，新 DLL 对老 Server 拿到的缺省是全拼的「拼」。
    #[serde(default)]
    pub glyph: ModeGlyph,
}

impl Default for InputSettings {
    fn default() -> Self {
        Self {
            switch_mode: SwitchKey::default(),
            english_mode: true,
            default_mode: DefaultMode::default(),
            zhuyin: false,
            shift_letter_compose: false,
            glyph: ModeGlyph::default(),
        }
    }
}

/// Server 发给 DLL 的消息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerMessage {
    /// 对一次 [`super::ClientMessage::OpenSession`] 的答复：把 DLL 在按键到达之前就要知道的
    /// 设置带过去一次（之后 [`Self::ModeSync`] 的每一拍也带着，改了配置不用重开会话）。
    ///
    /// **只回过协议版本对得上的 DLL**：老的 `open` 是只写不读，多回一条会被它当成下一次
    /// `Poll` 的应答而报错（见 Server 侧 `handle`）。
    SessionOpened {
        /// 会话标识。
        session: SessionId,

        /// 按键行为设置。
        input: InputSettings,
    },

    /// 对一次 [`super::ClientMessage::Key`] 的处理结果。
    KeyResult {
        /// 会话标识。
        session: SessionId,

        /// 这次按键吃掉还是放行。
        outcome: KeyOutcome,

        /// 本次要立即上屏的文本（选词 / 空格上屏 / 标点等）；没有则为 `None`。
        commit: Option<String>,

        /// 处理后要绘制的组句状态（preedit + 候选）；空 [`Frame`] 表示收起候选窗口。
        frame: Frame,
    },

    /// 对一次 [`super::ClientMessage::Commit`] 的答复：缓冲区里原样上屏的文本（拼音字母 / 英文模式下敲的字母）；
    /// 没在组句时为 `None`。Server 侧组句已清空，DLL 收到后把文本落进文档并收起组句。
    Committed {
        /// 会话标识。
        session: SessionId,

        /// 要原样上屏的文本。
        text: Option<String>,
    },

    /// 不由按键触发的重绘（云联想补词、本地整句模型重排到达）；也顺路带回用户在候选窗口上点选的候选。
    Update {
        /// 会话标识。
        session: SessionId,

        /// 要重绘的状态。
        frame: Frame,

        /// 用户点了候选窗里的候选、Server 已经替他把这个词选上：本次要立即上屏的文本。
        /// 点选发生在 DLL 没来问的时候，而传输是一问一答（Server 不主动推），所以攒到下一次 `Poll`
        /// 一起带回，最迟一拍。不认这个字段的老 DLL 当 `false`，只是少一次点选。
        #[serde(default)]
        commit: Option<String>,
    },

    /// 对一次 [`super::ClientMessage::SyncMode`] 的答复：状态条上点出来、还没被取走的目标模式，
    /// 外加当前的按键行为设置（每一拍都带，DLL 那边热加载就靠它）。
    ModeSync {
        /// 会话标识。
        session: SessionId,

        /// `Some(true)` 切英文、`Some(false)` 切中文；`None` 没有待处理的切换。
        english: Option<bool>,

        /// 按键行为设置；老 DLL 不认识这个字段，读到时忽略（serde 默认忽略多余字段）。
        #[serde(default)]
        input: InputSettings,
    },

    /// 收到「翻译选中文字」快捷键：请 DLL 在读编辑会话里取当前选区，用
    /// [`super::ClientMessage::Selection`] 回。这是对触发快捷键那次 [`super::ClientMessage::Key`] 的应答
    /// （替代常规 [`Self::KeyResult`]）；随后 DLL 发来的 `Selection` 才引出翻译候选帧。
    RequestSelection {
        /// 会话标识。
        session: SessionId,

        /// 请求标识，回时带上。
        request: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 线上格式是 JSON（[`super::codec`]），所以缺字段能靠 `serde(default)` 兜住：
    /// 老 Server 的 JSON 里没有 `zhuyin` / `default_mode` / `glyph`，新 DLL 读出来是缺省值而不是报错。
    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let old = r#"{"switch_mode":"shift","english_mode":true}"#;
        let input: InputSettings = serde_json::from_str(old).expect("老 Server 的帧要能读");
        assert_eq!(
            input,
            InputSettings {
                switch_mode: SwitchKey::Shift,
                ..InputSettings::default()
            }
        );
        assert!(!input.zhuyin);
        assert_eq!(input.glyph, ModeGlyph::Pinyin);
    }

    #[test]
    fn round_trips_through_json() {
        let input = InputSettings {
            zhuyin: true,
            glyph: ModeGlyph::Wubi,
            ..InputSettings::default()
        };
        let text = serde_json::to_string(&input).expect("写得出来");
        assert_eq!(
            serde_json::from_str::<InputSettings>(&text).expect("读得回来"),
            input
        );
    }

    /// 任务栏那张图一个字：五笔开着优先出「五」，否则按拼音侧的方案；两条轴都关退回「中」。
    #[test]
    fn mode_glyph_picks_one_letter_per_priority() {
        use qingjian_core::ShuangpinScheme;
        assert_eq!(
            ModeGlyph::for_scheme(Scheme::Pinyin, false),
            ModeGlyph::Pinyin
        );
        assert_eq!(
            ModeGlyph::for_scheme(Scheme::Shuangpin(ShuangpinScheme::Xiaohe), false),
            ModeGlyph::Shuangpin
        );
        assert_eq!(
            ModeGlyph::for_scheme(Scheme::Zhuyin, false),
            ModeGlyph::Zhuyin
        );
        // 五笔与拼音同时开着：出「五」
        assert_eq!(ModeGlyph::for_scheme(Scheme::Pinyin, true), ModeGlyph::Wubi);
        assert_eq!(
            ModeGlyph::for_scheme(Scheme::Shuangpin(ShuangpinScheme::Xiaolang), true),
            ModeGlyph::Wubi
        );
        // 两条轴都关
        assert_eq!(
            ModeGlyph::for_scheme(Scheme::Off, false),
            ModeGlyph::Chinese
        );
    }
}
