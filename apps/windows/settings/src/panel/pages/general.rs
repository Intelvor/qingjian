//! 「通用」页：学习语言、每页候选数、双拼、大千注音——怎么算、怎么打字的基础项。
//! 中英切换、标点、英文候选那些「输入行为」在 [`super::typing`]。

use qingjian_platform::MAX_PAGE_SIZE;
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

/// 双拼方案：界面名 + 配置写法（空串为全拼）。
pub(crate) const SHUANGPIN: [(&str, &str); 5] = [
    ("全拼（不启用双拼）", ""),
    ("小鹤双拼", "xiaohe"),
    ("自然码", "ziranma"),
    ("微软双拼", "microsoft"),
    ("搜狗双拼", "sogou"),
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
    ];
    page("通用", StackPanel::new().spacing(16.0).children(rows))
}
