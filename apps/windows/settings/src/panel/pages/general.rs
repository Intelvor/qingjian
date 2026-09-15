//! 「通用」页：学习语言、每页候选数、双拼、英文模式候选。

use qingjian_platform::{MAX_PAGE_SIZE, SwitchKey};
use windows_reactor::*;

use crate::panel::controls::{field, index_of, page};
use crate::panel::{Message, Settings};

/// 学习语言：界面名 + 配置写法。
pub(crate) const LANGUAGES: [(&str, &str); 3] =
    [("英语", "en"), ("日语", "ja"), ("西班牙语", "es")];

/// 双拼方案：界面名 + 配置写法（空串为全拼）。
pub(crate) const SHUANGPIN: [(&str, &str); 5] = [
    ("全拼（不启用双拼）", ""),
    ("小鹤双拼", "xiaohe"),
    ("自然码", "ziranma"),
    ("微软双拼", "microsoft"),
    ("搜狗双拼", "sogou"),
];

/// 中英切换键：界面名 + 配置写法，与 [`SwitchKey::ALL`] 同序（有测试钉住）。
pub(crate) const SWITCH_KEYS: [(&str, &str); 4] = [
    (SwitchKey::Shift.label(), SwitchKey::Shift.key()),
    (SwitchKey::Control.label(), SwitchKey::Control.key()),
    (SwitchKey::CtrlSpace.label(), SwitchKey::CtrlSpace.key()),
    (SwitchKey::None.label(), SwitchKey::None.key()),
];

fn string_combo(
    options: &'static [(&str, &str)],
    current: &str,
    callback: Callback<Option<usize>>,
) -> ComboBox {
    ComboBox::new()
        .items_source(options.iter().map(|(label, _)| *label))
        .selected_index(index_of(options, current))
        .on_selection_changed(callback)
}

pub(crate) fn view(settings: &Settings, context: &mut ViewContext<Settings>) -> View {
    let g = &settings.config.general;
    let english_off = !settings.config.apps.english_candidates_off.is_empty();
    let rows = [
        field(
            "学习语言",
            "候选词右侧显示哪种语言的译词，只列出装了释义表的语言。",
            string_combo(
                &LANGUAGES,
                &g.learning_language,
                context.callback(Message::LearningLanguage),
            ),
        ),
        field(
            "每页候选数",
            "",
            NumberBox::new()
                .minimum(1.0)
                .maximum(MAX_PAGE_SIZE as f64)
                .value(g.page_size as f64)
                .on_value_changed(context.callback(Message::PageSize)),
        ),
        field(
            "双拼",
            "开双拼后 v、u、i 是音节键，表达式与问字模式只能用 ? 开头进；微软、搜狗方案的 ; 键是 ing。",
            string_combo(
                &SHUANGPIN,
                &g.shuangpin,
                context.callback(Message::Shuangpin),
            ),
        ),
        field(
            "大千注音",
            "启用大千注音键盘布局（容错设定如 ㄢㄤ、ㄣㄥ 不分，请至「模糊音」分页开启）。",
            ToggleSwitch::new()
                .is_on(g.zhuyin)
                .on_toggled(context.callback(Message::Zhuyin)),
        ),
        field(
            "中文模式标点转全角",
            "没在打拼音时敲 , . ? ! 等出「，。？！」，数字后面的点保持半角；悬浮状态条的「，。」格也能切，切的是当前模式那份。",
            ToggleSwitch::new()
                .is_on(g.full_width_punctuation)
                .on_toggled(context.callback(Message::FullWidthPunctuation)),
        ),
        field(
            "英文模式标点转全角",
            "中英各记一份，缺省英文半角。",
            ToggleSwitch::new()
                .is_on(g.english_full_width_punctuation)
                .on_toggled(context.callback(Message::EnglishFullWidthPunctuation)),
        ),
        field(
            "英文模式（Caps Lock）也给候选",
            "Tab 或方向键选词；空格、回车、标点仍原样上屏敲的字母，不选词时与直接打字一样。",
            ToggleSwitch::new()
                .is_on(g.english_candidates)
                .on_toggled(context.callback(Message::EnglishCandidates)),
        ),
        field(
            "但在终端和代码编辑器里不给",
            "终端、Windows Terminal、VS Code、Cursor、JetBrains 等，那里的候选窗口会挡住应用自己的补全；名单可在配置文件里改。",
            ToggleSwitch::new()
                .is_on(english_off)
                .is_enabled(g.english_candidates)
                .on_toggled(context.callback(Message::EnglishOffInApps)),
        ),
        field(
            "中英切换键",
            "单击选中的键（或按 Ctrl + Space）在中英之间切换，改完立刻生效。打字时容易误触 Shift 的话改成「单击 Ctrl」；「不切换」时只剩任务栏 / 悬浮状态条上的「中」「英」按钮。注意 Ctrl + Space 常被编辑器用作代码补全等快捷键，选了它会把这些应用里的该组合键抢过来。",
            string_combo(
                &SWITCH_KEYS,
                settings.config.shortcut.switch_mode.key(),
                context.callback(Message::SwitchMode),
            ),
        ),
        field(
            "启用内置英文模式",
            "关掉后青简固定中文模式：切换键与任务栏、悬浮状态条上的「中」「英」按钮都不再切到英文，需要英文时用系统快捷键（Win + Space）切到别的输入法。",
            ToggleSwitch::new()
                .is_on(g.english_mode)
                .on_toggled(context.callback(Message::EnglishMode)),
        ),
    ];
    page("通用", StackPanel::new().spacing(16.0).children(rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 下拉的项与配置枚举一一对应，顺序也一样（下标就是 `SwitchKey::ALL` 的下标）。
    #[test]
    fn switch_key_options_follow_the_config_enum() {
        assert_eq!(SWITCH_KEYS.len(), SwitchKey::ALL.len());
        for (index, key) in SwitchKey::ALL.into_iter().enumerate() {
            assert_eq!(SWITCH_KEYS[index], (key.label(), key.key()));
        }
    }
}
