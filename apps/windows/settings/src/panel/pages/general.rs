//! 「通用」页：学习语言、每页候选数、输入方案（拼音 / 五笔）、繁体输出——怎么算、怎么打字的基础项。
//! 中英切换、标点、英文候选那些「输入行为」在 [`super::typing`]（上游把它们都放在这一页，本仓库分开了）。

use qingjian_platform::{MAX_PAGE_SIZE, Scheme};
use windows_reactor::*;

use crate::panel::controls::{field, index_of, page};
use crate::panel::{Message, Settings};

/// 学习语言：界面名 + 配置写法。
pub(crate) const LANGUAGES: [(&str, &str); 4] = [
    ("英语", "en"),
    ("日语", "ja"),
    ("西班牙语", "es"),
    ("不显示译文", "off"),
];

/// 输入方案：界面名 + 配置写法，直接照 [`Scheme::ALL`] 建，不另抄一份。
/// 数组长度取自 `ALL`，以后加方案时这里数组对不上就编不过。
pub(crate) const SCHEMES: [(&str, &str); Scheme::ALL.len()] = [
    (Scheme::ALL[0].label(), Scheme::ALL[0].key()),
    (Scheme::ALL[1].label(), Scheme::ALL[1].key()),
    (Scheme::ALL[2].label(), Scheme::ALL[2].key()),
    (Scheme::ALL[3].label(), Scheme::ALL[3].key()),
    (Scheme::ALL[4].label(), Scheme::ALL[4].key()),
    (Scheme::ALL[5].label(), Scheme::ALL[5].key()),
    (Scheme::ALL[6].label(), Scheme::ALL[6].key()),
    (Scheme::ALL[7].label(), Scheme::ALL[7].key()),
    (Scheme::ALL[8].label(), Scheme::ALL[8].key()),
    (Scheme::ALL[9].label(), Scheme::ALL[9].key()),
];

pub(crate) fn string_combo(
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
    let rows = [
        field(
            "学习语言",
            "候选词右侧显示哪种语言的译词，只列出装了释义表的语言；「不显示译文」同时关掉生词标记与释义兜底。",
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
            "拼音方案",
            "全拼、五套双拼、大千注音，或关（只用下面的五笔）。\
             双拼下 v、u、i 是音节键，表达式与问字模式改用 Shift+V、Shift+U 进（微软、搜狗方案的 ; 键是 ing）；\
             注音下 v、u、i 也是按键，只能用 ? 开头进。",
            string_combo(
                &SCHEMES,
                g.scheme().key(),
                context.callback(Message::Scheme),
            ),
        ),
        field(
            "五笔（86 版）",
            "与拼音方案同时开着就是混输：编码打全的五笔词在前，打不出的字直接打拼音。\
             单用五笔请把拼音方案关掉；第 5 个字母起五笔查不到东西，自动只剩拼音。\
             译词、生词记录与学习照常。",
            ToggleSwitch::new()
                .is_on(g.wubi())
                .on_toggled(context.callback(Message::Wubi)),
        ),
        field(
            "繁体输出",
            "打字时将候选词转换为繁体中文。",
            ToggleSwitch::new()
                .is_on(g.traditional)
                .on_toggled(context.callback(Message::Traditional)),
        ),
    ];
    page("通用", StackPanel::new().spacing(16.0).children(rows))
}
