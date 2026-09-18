//! 「输入行为」页：中英怎么切、字母标点怎么收，以及中文 / 英文候选谁排前面。

use qingjian_platform::{DefaultMode, ShiftLetter, SwitchKey};
use windows_reactor::*;

use crate::panel::controls::{field, index_of, page};
use crate::panel::{Message, Settings};

/// 中英切换键：界面名 + 配置写法，与 [`SwitchKey::ALL`] 同序（有测试钉住）。
pub(crate) const SWITCH_KEYS: [(&str, &str); 3] = [
    (SwitchKey::Shift.label(), SwitchKey::Shift.key()),
    (SwitchKey::Control.label(), SwitchKey::Control.key()),
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

/// 按 [`DefaultMode::ALL`] 列项的「新窗口默认模式」下拉；顺序与它一致（有测试钉住）。
fn default_mode_combo(current: DefaultMode, callback: Callback<Option<usize>>) -> ComboBox {
    ComboBox::new()
        .items_source(DefaultMode::ALL.iter().map(|mode| mode.label()))
        .selected_index(DefaultMode::ALL.iter().position(|mode| *mode == current))
        .on_selection_changed(callback)
}

/// Shift+字母的下拉：选项直接由 [`ShiftLetter::ALL`] 生成，免得再抄一份表（顺序要和它一致）。
fn shift_letter_combo(current: ShiftLetter, callback: Callback<Option<usize>>) -> ComboBox {
    ComboBox::new()
        .items_source(ShiftLetter::ALL.iter().map(|mode| mode.label()))
        .selected_index(ShiftLetter::ALL.iter().position(|mode| *mode == current))
        .on_selection_changed(callback)
}

pub(crate) fn view(settings: &Settings, context: &mut ViewContext<Settings>) -> View {
    let g = &settings.config.general;
    let english_off = !settings.config.apps.english_candidates_off.is_empty();
    let rows = [
        field(
            "中英切换键",
            "单击选中的键在中英之间切换，改完立刻生效。打字时容易误触 Shift 的话改成「单击 Ctrl」；「不切换」时只剩任务栏 / 悬浮状态条上的「中」「英」按钮。",
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
        field(
            "新窗口的输入模式",
            "新窗口（新的一条输入线程）激活时用哪种模式：「记住上次」（缺省）沿用系统记住的那份；\
             选中文 / 英文就在激活时设成它。窗口里手动切过的模式在切走再切回时会被重置。",
            default_mode_combo(g.default_mode, context.callback(Message::DefaultMode)),
        ),
        field(
            "中文模式下的 Shift + 字母",
            "「交给应用」是临时打英文（与以前一致）：拼音先上屏，这个键归应用；\
             「进组句」把它收进拼音缓冲区，匹配时按小写算，所以 Cpan 与 cpan 一样能出「C盘」。",
            shift_letter_combo(g.shift_letter, context.callback(Message::ShiftLetter)),
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
            "输入拼音时中文候选排在英文词前面",
            "开着时整段输入是英文词时（hello、key）英文词排第二，空格上屏的仍是中文；关着（缺省）拼音不成立的输入英文词排第一。",
            ToggleSwitch::new()
                .is_on(g.chinese_first)
                .on_toggled(context.callback(Message::ChineseFirst)),
        ),
    ];
    page("输入行为", StackPanel::new().spacing(16.0).children(rows))
}
