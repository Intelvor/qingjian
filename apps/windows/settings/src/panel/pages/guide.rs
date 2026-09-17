//! 「使用说明」页：按键速查。
//!
//! 键名跟着当前配置渲染——翻页键对、译词 / 删候选的修饰键、翻译组合键、前缀键
//! （表达式 / 问字 / 续写）、中英切换键、注音开关，都会按「通用」「输入行为」「快捷键」
//! 「云服务」页里选的那些变。**双拼下前缀键要先按住 Shift**（`v` `u` `i` 在那几套方案里
//! 都是音节键），也按「通用」页选的方案渲染。参考文档见仓库 `docs/user/getting-started/keys.md`。

use qingjian_platform::{KeyCombo, Modifiers, SwitchKey};
use windows_reactor::*;

use crate::panel::Settings;
use crate::panel::controls::{note, page};

use super::cloud::SENTENCE_TRIGGERS;

/// 一节：小标题 + 若干「键 → 说明」行。
fn section(title: &str, rows: Vec<View>) -> View {
    let heading: View = TextBlock::new()
        .text(title)
        .font_size(15.0)
        .font_weight(FontWeight::SEMI_BOLD)
        .into();
    let items = std::iter::once(heading)
        .chain(rows)
        .enumerate()
        .map(|(index, view)| KeyedView::new(index.to_string(), view));
    StackPanel::new().spacing(6.0).keyed_children(items)
}

/// 一行：左边固定宽的键名，右边说明。
fn key_row(keys: impl Into<String>, what: impl Into<String>) -> View {
    let keys: String = keys.into();
    let what: String = what.into();
    let label: View = TextBlock::new().text(keys).width(170.0).into();
    let text: View = TextBlock::new()
        .text(what)
        .text_wrapping(TextWrapping::Wrap)
        .into();
    StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(12.0)
        .children([label, text])
}

/// 修饰键的 Windows 键名。Core 里存的是 macOS 的叫法（control / option / command），
/// 界面上按 Windows 的习惯写（Ctrl / Alt / Win）。
fn modifiers_name(modifiers: Modifiers) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if modifiers.control {
        parts.push("Ctrl");
    }
    if modifiers.shift {
        parts.push("Shift");
    }
    if modifiers.option {
        parts.push("Alt");
    }
    if modifiers.command {
        parts.push("Win");
    }
    if parts.is_empty() {
        return "（没配）".to_owned();
    }
    parts.join(" + ")
}

/// 「Ctrl + 数字」。
fn with_digit(modifiers: Modifiers) -> String {
    format!("{} + 数字", modifiers_name(modifiers))
}

/// 翻译选中文字那个组合键。
fn combo_name(combo: KeyCombo) -> String {
    format!(
        "{} + {}",
        modifiers_name(combo.modifiers),
        combo.key.to_ascii_uppercase()
    )
}

/// 翻页键对的名字。
fn page_keys_name(page_keys: &str) -> &'static str {
    match page_keys {
        ",." => "逗号、句号",
        "-=" => "减号、等号",
        _ => "方括号 [ ]",
    }
}

/// 前缀键的名字：双拼下 `v` / `u` / `i` 都是音节键，要按住 Shift 敲大写（见 `ModeKeys::shifted`）。
fn prefix_name(key: char, shuangpin: bool) -> String {
    if shuangpin {
        format!("Shift + {}", key.to_ascii_uppercase())
    } else {
        key.to_string()
    }
}

pub(crate) fn view(settings: &Settings, _context: &mut ViewContext<Settings>) -> View {
    let g = &settings.config.general;
    let s = &settings.config.shortcut;
    let p = &settings.config.predict;
    let m = s.mode;
    let shuangpin = g.shuangpin().is_some();
    let (first_translation, second_translation) = s.translation_keys();
    let delete_candidate = s.delete_keys();

    // 云端整句那两下 Tab 干什么，看「云服务」页选的是哪种时机。
    let trigger = SENTENCE_TRIGGERS
        .iter()
        .find(|(_, key)| *key == p.sentence_trigger.key())
        .map_or(SENTENCE_TRIGGERS[0].0, |(name, _)| *name);
    let tab_hint = if p.sentence_trigger.key() == "tab" {
        format!(
            "请云端按你写的内容补一句：按一下开始算（候选窗先摆「☁ …」），算好了再按一下采用。\
             整句补全现在是「{trigger}」。"
        )
    } else {
        format!(
            "接受云端补全的整句；没有补全时这一下交给应用（缩进、跳焦点）。\
             整句补全现在是「{trigger}」。"
        )
    };
    let cloud_hint = if p.enabled {
        "云联想现在开着。"
    } else {
        "需要先在「云服务」页打开云联想。"
    };

    let mut sections: Vec<View> = Vec::new();

    sections.push(section(
        "打字",
        vec![
            key_row(
                "字母",
                format!(
                    "输入拼音，候选窗在光标下方出现。拼音行显示在哪由「候选窗口」页的「拼音显示位置」定，现在是「{}」。",
                    g.preedit.label()
                ),
            ),
            key_row("空格", "上屏高亮那个候选；没在组句时就是空格。"),
            key_row(
                "1 – 9",
                format!(
                    "上屏当前页第 N 个候选（每页 {} 个，「通用」页可改）；当前页没有第 N 个时，\
                     这个数字当内容接在字母后面（敲 gpt6 就出 gpt6）。",
                    g.page_size
                ),
            ),
            key_row("↑ ↓", "移动高亮，到页边自动翻页。"),
            key_row(
                page_keys_name(&g.page_keys),
                "翻页；PageUp / PageDown 也行。翻页键对在「快捷键」页改。",
            ),
            key_row("← →", "移动拼音光标：候选只按光标之前的拼音算。"),
            key_row("Backspace", "删掉光标前一个字母。"),
            key_row("Esc", "清空这次输入。"),
            key_row("Enter", "把拼音原样上屏（不想打中文时用）。"),
            key_row("Tab", tab_hint),
        ],
    ));

    let switch_hint = match s.switch_mode {
        SwitchKey::None => {
            "现在设成「不切换」：要打英文请按 Win + Space 切到别的输入法。可在「输入行为」页改。"
        }
        _ => {
            "按一下在中文与英文之间切换；任务栏与悬浮状态条的「中 / 英」也能点。\
             可在「输入行为」页改成单击 Ctrl 或不切换。"
        }
    };
    let english_hint = if g.english_mode {
        "Caps Lock 亮着就是英文模式，其中大小写照敲；关掉「内置英文模式」后英文模式不再可选。"
    } else {
        "「内置英文模式」现在是关的，Caps Lock 不再切英文，这一路用不上。"
    };
    let shift_hint = if g.shift_letter.compose() {
        "大写按小写参与匹配（Cpan 与 cpan 一样出「C盘」）。可在「输入行为」页改回「交给应用」。"
    } else {
        "拼音先原样上屏，这个大写字母交给应用。可在「输入行为」页改成「进组句」。"
    };
    let punctuation_hint = if settings.config.status_bar.enabled {
        format!(
            "点悬浮状态条上的「，。」格切换全角 / 半角（中英各记一份，现在是中文{}、英文{}）。",
            if g.full_width_punctuation {
                "全角"
            } else {
                "半角"
            },
            if g.english_full_width_punctuation {
                "全角"
            } else {
                "半角"
            }
        )
    } else {
        "悬浮状态条现在是关的；全角 / 半角可以在「输入行为」页改。".to_owned()
    };
    sections.push(section(
        "中英与标点",
        vec![
            key_row(s.switch_mode.label(), switch_hint),
            key_row("Caps Lock", english_hint),
            key_row("Shift + 字母", shift_hint),
            key_row("状态条「，。」", punctuation_hint),
        ],
    ));

    // 英文模式下选词与翻页跟中文一样（1.0.6 起）：空格选高亮、数字选第 N 个、翻页键翻页。
    // 「内置英文模式」关着时这一路用不上，但照「全貌」的规矩仍整节列出，只在末尾说明。
    let mut english_rows = vec![
        key_row("空格 / Tab", "上屏高亮那个候选；没有候选时就是空格。"),
        key_row(
            "1 – 9",
            "选当前页第 N 个候选；当前页没有第 N 个时，这个数字当内容接在字母后面（敲 gpt6 就出 gpt6）。",
        ),
        key_row(
            format!("{}、↑ ↓", page_keys_name(&g.page_keys)),
            "翻页 / 移动高亮，到页边自动翻页。翻页键对在「快捷键」页改。",
        ),
        key_row(
            "Enter",
            "把所敲的字母原样上屏；带数字的标识符可以先 Enter 输出字母再敲数字。",
        ),
    ];
    english_rows.push(note(if g.english_mode {
        "Caps Lock 亮着就是英文模式，其中大小写照敲；选词、翻页与中文模式一致。"
    } else {
        "「内置英文模式」现在关着，这一节用不上；要打英文请按 Win + Space 切到别的输入法。"
    }));
    sections.push(section("英文模式", english_rows));

    let question_hint = format!("用拼音问一个字或一个短问题（需要云联想）。{cloud_hint}");
    let continue_hint = format!(
        "让云端接着光标前后的文字往下写一段（前缀本身不上屏），再按一次 Tab 采用。{cloud_hint}"
    );
    let mut prefix_rows = vec![
        key_row("rq / sj / xq", "日期 / 时间 / 星期。"),
        key_row(
            format!("{} + 算式或数字", prefix_name(m.expression, shuangpin)),
            "算了算式、出中文数字与金额（1+2 出 3，123 出一百二十三元整，\
             123.5 出一百二十三点五 / 壹佰贰拾叁元伍角）。",
        ),
        key_row(
            format!("{} + 四位数", prefix_name(m.question, shuangpin)),
            "出该 Unicode 码点对应的字符（4e00 出「一」）。",
        ),
        key_row(
            format!("{} + 拼音", prefix_name(m.question, shuangpin)),
            question_hint,
        ),
        key_row(
            format!("{} 再按 Tab", prefix_name(m.continue_key, shuangpin)),
            continue_hint,
        ),
    ];
    if m.question_mark {
        prefix_rows.push(key_row(
            "? + 拼音",
            "没在输入拼音时敲 ? 也进问字；后面跟的不是字母时会还原成问号。",
        ));
    }
    // 双拼下这三个字母是音节键，前缀要按住 Shift；注音下它们被大千布局占作音符键，带前缀的都用不了。
    let prefix_note = if g.zhuyin {
        "前缀键在「快捷键」页改，三个键不能相同。注音下 v / u / i 都是键盘布局里的音符键，这三条前缀用不了。"
    } else if shuangpin {
        "前缀键在「快捷键」页改，三个键不能相同；双拼下要先按住 Shift，因为 v / u / i 在双拼里都是音节键。"
    } else {
        "前缀键在「快捷键」页改，三个键不能相同。"
    };
    prefix_rows.push(note(prefix_note));
    sections.push(section("前缀键", prefix_rows));

    sections.push(section(
        "译词与删除候选",
        vec![
            key_row(with_digit(first_translation), "上屏候选右侧的第一条译词。"),
            key_row(
                with_digit(second_translation),
                "候选有两条译词时，上屏第二条。",
            ),
            key_row(
                with_digit(delete_candidate),
                "删掉第 N 个候选：自己造的词整个删掉，词库里的词清掉学习记录。",
            ),
            note("三组键在「快捷键」页改。Alt + 数字会被应用当菜单快捷键，缺省用 Ctrl。"),
        ],
    ));

    sections.push(section(
        "云联想",
        vec![
            key_row(
                combo_name(s.translate_selection),
                format!("翻译应用里选中的文字（中译外 / 外译中）。{cloud_hint}"),
            ),
            key_row("Enter / 空格 / 1", "翻译结果替换选区；Esc 保留原文。"),
        ],
    ));

    if g.zhuyin {
        sections.push(section(
            "注音模式（已开启）",
            vec![
                key_row("Enter", "上屏高亮候选——数字键都被注音符号占了，选词靠它。"),
                key_row("Shift + Enter", "把注音符号原样上屏。"),
                key_row("空格", "一声 / 轻声。"),
                key_row(
                    "数字、- ; , . /",
                    "进组句当声调与符号，不选词。翻页仍用上面的翻页键。",
                ),
            ],
        ));
    }

    if settings.config.status_bar.enabled {
        sections.push(section(
            "悬浮状态条",
            vec![
                key_row("「中 / 英」", "点一下切换中英模式。"),
                key_row("「，。」", "点一下切换全角 / 半角标点。"),
                key_row("齿轮", "打开这个设置窗口。"),
                note("可以拖到屏幕任意位置；不想要了在「候选窗口」页关掉。"),
            ],
        ));
    }

    let scheme = if g.zhuyin {
        "大千注音".to_owned()
    } else {
        g.shuangpin().map_or("全拼".to_owned(), |scheme| {
            format!("双拼（{}）", scheme.label())
        })
    };
    sections.push(section(
        "现在的输入方案",
        vec![
            key_row("输入方案", scheme),
            key_row(
                "繁体输出",
                if g.traditional {
                    "开着：上屏的是繁体，词库与学习记录仍是简体。"
                } else {
                    "关着：上屏简体。"
                },
            ),
            key_row(
                "中英混输中文优先",
                if g.chinese_first {
                    "开着：整段是英文词时中文候选也排第一。"
                } else {
                    "关着：拼音不像话的输入（hello）英文词排第一。"
                },
            ),
            key_row(
                "云联想",
                if p.enabled {
                    "开着：停顿或按 Tab 时会请云端。"
                } else {
                    "关着：全部在本机算。"
                },
            ),
        ],
    ));

    sections.push(note(
        "这份速查跟着你的设置变：翻页键、译词 / 删候选的修饰键、前缀键、中英切换键、注音开关改了，\
         这里显示的按键与说明会跟着换。更详细的说明见随包的文档与官网。",
    ));

    let body = StackPanel::new().spacing(20.0).keyed_children(
        sections
            .into_iter()
            .enumerate()
            .map(|(index, view)| KeyedView::new(index.to_string(), view)),
    );

    page("使用说明", body)
}
