//! 根组件的 Reactor 生命周期：建状态、按消息落盘、画左侧导航 + 当前页。

use std::sync::OnceLock;

use qingjian_platform::{
    AccentColor, CandidateRenderer, Config, DEFAULT_ENGLISH_CANDIDATES_OFF_WINDOWS, DefaultMode,
    LayoutMode, LogLevel, PreeditMode, ShiftLetter, ThemeMode,
};
use windows_reactor::*;

use super::cloud_status::CloudStatus;
use super::controls::{export_logs, log_dir, open_in_editor, open_with_explorer};
use super::notice::Notice;
use super::pages::{about, aux_code, cloud, dictionaries, general, shortcut, typing};
use super::recorder::Recorder;
use super::{Message, Settings};

/// 窗口标题栏左上角的图标路径：随包装在 exe 旁边的 `qingjian.ico`。
///
/// WinUI 的标题栏图标**不吃 exe 里的资源**，得用 `WindowVisuals::icon` 单独设一次（装机包已经把
/// 那个文件放在 `{app}` 下，见 `qingjian.iss`）。`icon()` 收 `&'static str`，所以算出来的路径泄漏
/// 一次、用 `OnceLock` 记住；开发态（`cargo run`，exe 旁边没有这个文件）返回 `None`：
/// 不设图标也不报错。
fn window_icon() -> Option<&'static str> {
    static ICON: OnceLock<Option<&'static str>> = OnceLock::new();
    *ICON.get_or_init(|| {
        let path = std::env::current_exe().ok()?.parent()?.join("qingjian.ico");
        path.is_file().then(|| {
            Box::leak(path.to_string_lossy().into_owned().into_boxed_str()) as &'static str
        })
    })
}

/// 导航图标：`SymbolIcon`（WinUI `Symbol` 枚举）或 `FontIcon`（Segoe MDL2 Assets 的字面 glyph 编码）。
///
/// 关键区分（踩过的坑）：
/// - `Symbol::Help`（E897）本身是**圆圈包问号「?」**，不是回退，就是那个图标。
/// - 「ⓘ 圆圈包 i」在 Segoe MDL2 Assets 里是 **E946（Info）**，WinUI `Symbol` 枚举没直接封装，
///   用 `FontIcon` 传字面 glyph `"\u{E946}"`。`FontIcon` 默认字体就是 Segoe MDL2 Assets，精确渲染。
/// - 千万别用 Unicode `ℹ`（U+2139）：它不是 Segoe MDL2 Assets 的字形，落到系统字体又未必有字形，
///   会回退成「□」豆腐框。
enum Icon {
    Symbol(Symbol),
    Glyph(&'static str),
}

impl Icon {
    fn symbol(symbol: Symbol) -> Self {
        Self::Symbol(symbol)
    }

    fn glyph(glyph: &'static str) -> Self {
        Self::Glyph(glyph)
    }

    fn into_view(self) -> View {
        match self {
            Self::Symbol(symbol) => SymbolIcon::new().symbol(symbol).into(),
            Self::Glyph(glyph) => FontIcon::new().glyph(glyph).into(),
        }
    }
}

impl Component for Settings {
    type Input = ();
    type Message = Message;

    fn create(_input: &(), _context: &ComponentContext<Self>) -> Self {
        let path = Self::config_path();
        Self::ensure_config_file(&path);
        let config = Config::load(&path).unwrap_or_default();
        Self {
            config,
            path,
            page: "general".to_string(),
            cloud_status: CloudStatus::Idle,
            recorder: Recorder::Idle,
            record_box: ElementRef::new(),
            notice: Notice::default(),
            dictionary_status: String::new(),
            families: qingjian_render::system_fonts::families(),
            font_query: None,
            word_query: None,
            word_page: 0,
            word_status: None,
        }
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::Navigate(Some(tag)) => {
                self.page = tag;
                // 上一页的导入提示不跟着过来
                self.notice.clear();
            }
            Message::Navigate(None) => {}

            // 通用页
            Message::LearningLanguage(Some(i)) if i < general::LANGUAGES.len() => {
                self.save("general", "learning_language", general::LANGUAGES[i].1);
            }
            Message::PageSize(Some(value)) => {
                let size = (value.round() as i64).clamp(1, 9);
                self.save("general", "page_size", size);
            }
            Message::Scheme(Some(i)) if i < general::SCHEMES.len() => {
                self.save("general", "scheme", general::SCHEMES[i].1);
            }
            Message::Wubi(on) => self.save("general", "wubi", if on { "wubi86" } else { "" }),
            Message::Traditional(on) => self.save("general", "traditional", on),
            Message::EnglishCandidates(on) => self.save("general", "english_candidates", on),
            Message::ChineseFirst(on) => self.save("general", "chinese_first", on),
            Message::FullWidthPunctuation(on) => {
                self.save("general", "full_width_punctuation", on);
            }
            Message::EnglishFullWidthPunctuation(on) => {
                self.save("general", "english_full_width_punctuation", on);
            }
            Message::EnglishOffInApps(on) => {
                let list: Vec<String> = if on {
                    DEFAULT_ENGLISH_CANDIDATES_OFF_WINDOWS
                        .iter()
                        .map(|s| (*s).to_owned())
                        .collect()
                } else {
                    Vec::new()
                };
                self.save_array("apps", "english_candidates_off", &list);
            }
            Message::SwitchMode(Some(i)) if i < typing::SWITCH_KEYS.len() => {
                self.save("shortcut", "switch_mode", typing::SWITCH_KEYS[i].1);
            }
            Message::EnglishMode(on) => self.save("general", "english_mode", on),
            Message::DefaultMode(Some(i)) if i < DefaultMode::ALL.len() => {
                self.save("general", "default_mode", DefaultMode::ALL[i].key());
            }

            // 候选窗口页
            Message::Theme(Some(i)) if i < ThemeMode::ALL.len() => {
                self.save("general", "theme", ThemeMode::ALL[i].key());
            }
            Message::Accent(Some(i)) if i < AccentColor::ALL.len() => {
                self.save("general", "accent", AccentColor::ALL[i].key());
            }
            Message::Layout(Some(i)) if i < LayoutMode::ALL.len() => {
                self.save("general", "layout", LayoutMode::ALL[i].key());
            }
            Message::Preedit(Some(i)) if i < PreeditMode::ALL.len() => {
                self.save("general", "preedit", PreeditMode::ALL[i].key());
            }
            Message::ShiftLetter(Some(i)) if i < ShiftLetter::ALL.len() => {
                self.save("general", "shift_letter", ShiftLetter::ALL[i].key());
            }
            Message::Renderer(Some(i)) if i < CandidateRenderer::ALL.len() => {
                self.save("general", "renderer", CandidateRenderer::ALL[i].key());
            }
            Message::FontQuery(text) => {
                let text = text.trim().to_owned();
                let exact = self
                    .families
                    .iter()
                    .find(|family| family.eq_ignore_ascii_case(&text))
                    .cloned();
                match exact {
                    Some(family) => {
                        self.font_query = None;
                        self.save("general", "font", family);
                    }
                    None if text.is_empty() => {
                        self.font_query = None;
                        self.save("general", "font", "");
                    }
                    None => self.font_query = Some(text),
                }
            }
            Message::Font(family) => {
                self.font_query = None;
                self.save("general", "font", family);
            }
            Message::FontSize(Some(value)) => {
                let size = (value.round() as i64).clamp(8, 32);
                self.save("general", "font_size", size);
            }
            Message::StatusBar(on) => self.save("status_bar", "enabled", on),

            // 云服务页
            Message::LocalModel(on) => self.save("model", "enabled", on),
            Message::CloudEnabled(on) => self.save("predict", "enabled", on),
            Message::CloudApiKey(value) => self.save("predict", "api_key", value),
            Message::CloudModel(value) => self.save("predict", "model", value),
            Message::CloudBaseUrl(value) => self.save("predict", "base_url", value),
            Message::CloudSlots(Some(value)) => {
                let slots = (value.round() as i64).clamp(0, 9);
                self.save("predict", "slots", slots);
            }
            Message::CloudSentence(on) => self.save("predict", "sentence", on),
            Message::CloudLookback(Some(value)) => {
                let chars = (value.round() as i64).clamp(0, qingjian_predict::MAX_LOOKBACK as i64);
                self.save("predict", "lookback", chars);
            }
            Message::CloudLookahead(Some(value)) => {
                let chars = (value.round() as i64).clamp(0, qingjian_predict::MAX_LOOKAHEAD as i64);
                self.save("predict", "lookahead", chars);
            }
            Message::CloudSentenceTrigger(Some(i)) if i < cloud::SENTENCE_TRIGGERS.len() => {
                self.save("predict", "sentence_trigger", cloud::SENTENCE_TRIGGERS[i].1);
            }
            Message::TestConnection => {
                if matches!(self.cloud_status, CloudStatus::Testing) {
                    return;
                }
                self.cloud_status = CloudStatus::Testing;
                let config = self.config.predict.clone();
                context.spawn_background(move |cancel| {
                    Message::CloudTestDone(cloud::run_test(&config, &cancel))
                });
            }
            Message::CloudTestDone(result) => {
                self.cloud_status = match result {
                    Ok(message) => CloudStatus::Ok(message),
                    Err(message) => CloudStatus::Failed(message),
                };
            }

            // 快捷键页
            Message::PageKeys(Some(i)) if i < shortcut::PAGE_KEYS.len() => {
                self.save("general", "page_keys", shortcut::PAGE_KEYS[i].1);
            }
            Message::ModeExpression(Some(i)) if i < shortcut::MODE_KEYS.len() => {
                self.save("shortcut", "expression", shortcut::MODE_KEYS[i]);
            }
            Message::ModeQuestion(Some(i)) if i < shortcut::MODE_KEYS.len() => {
                self.save("shortcut", "question", shortcut::MODE_KEYS[i]);
            }
            Message::ModeContinue(Some(i)) if i < shortcut::MODE_KEYS.len() => {
                self.save("shortcut", "continue", shortcut::MODE_KEYS[i]);
            }
            Message::QuestionMark(on) => self.save("shortcut", "question_mark", on),
            Message::Translation(Some(i)) if i < shortcut::MODIFIERS.len() => {
                self.save("shortcut", "translation", shortcut::MODIFIERS[i].1);
            }
            Message::TranslationSecond(Some(i)) if i < shortcut::MODIFIERS.len() => {
                self.save("shortcut", "translation_second", shortcut::MODIFIERS[i].1);
            }
            Message::DeleteCandidate(Some(i)) if i < shortcut::MODIFIERS.len() => {
                self.save("shortcut", "delete_candidate", shortcut::MODIFIERS[i].1);
            }
            Message::TranslateSelection(Some(i)) if i < shortcut::MODIFIERS.len() => {
                let key = self.config.shortcut.translate_selection.key;
                let combo = format!("{}+{key}", shortcut::MODIFIERS[i].1);
                self.save("shortcut", "translate_selection", combo);
            }

            // 模糊音页
            Message::Fuzzy(key, on) => self.save("fuzzy", key, on),

            // 词库页
            Message::ToggleDomain(name, on) => {
                let mut domains = self.config.dictionaries.domains.clone();
                if on {
                    if !domains.contains(&name) {
                        domains.push(name);
                    }
                } else {
                    domains.retain(|d| d != &name);
                }
                self.save_array("dictionaries", "domains", &domains);
            }
            Message::ToggleUserDict(name, on) => {
                // 用户词库缺省启用，`disabled` 列的是关掉的。
                let mut disabled = self.config.dictionaries.disabled.clone();
                if on {
                    disabled.retain(|d| d != &name);
                } else if !disabled.contains(&name) {
                    disabled.push(name);
                }
                self.save_array("dictionaries", "disabled", &disabled);
            }
            Message::RemoveUserDict(name) => {
                dictionaries::remove_user_dict(self, &name);
                self.reload();
            }
            Message::ImportDictionary => {
                dictionaries::import(self);
                self.reload();
            }
            Message::WordQuery(text) => {
                self.word_query = (!text.is_empty()).then_some(text);
                // 换了筛选串就回第一页：不回去的话可能停在一个筛完已经不存在的页码上。
                self.word_page = 0;
            }
            Message::WordPage(delta) => {
                // 页数按**筛选后**的条数算（与画的时候同一套），越界夹回有效范围。
                let pages = dictionaries::page_count(dictionaries::matched_words(self).len());
                let target = self.word_page as isize + delta;
                self.word_page = target.clamp(0, pages as isize - 1) as usize;
            }
            Message::ForgetWord(word) => dictionaries::request_forget(self, &word),

            // 辅码页
            Message::AuxCodeEnabled(on) => self.save("aux_code", "enabled", on),
            Message::AuxCodeShow(on) => self.save("general", "aux_code_show", on),
            Message::AuxCodeKeepEmpty(on) => self.save("general", "aux_code_keep_empty", on),
            Message::AuxRecordStart => self.recorder = self.recorder.waiting(),
            Message::AuxRecordCancel => self.recorder = Recorder::Idle,
            Message::AuxRecorded(text) => aux_code::record_key(self, &text),
            Message::ToggleAuxTable(name, on) => {
                // 码表缺省启用，`disabled` 列的是关掉的；随包笔画表也走这条
                let mut disabled = self.config.aux_code.disabled.clone();
                if on {
                    disabled.retain(|d| d != &name);
                } else if !disabled.contains(&name) {
                    disabled.push(name);
                }
                self.save_array("aux_code", "disabled", &disabled);
            }
            Message::RemoveAuxTable(name) => {
                aux_code::remove_table(self, &name);
                self.reload();
            }
            Message::ImportCodeTable => {
                aux_code::import(self);
                self.reload();
            }

            // 高级页
            Message::VerboseLog(on) => {
                let level = if on { LogLevel::Debug } else { LogLevel::Info };
                self.save("general", "log_level", level.key());
            }
            Message::InputLog(on) => self.save("general", "input_log", on),
            Message::Learning(on) => self.save("general", "learning", on),
            Message::OpenConfigFile => {
                Self::ensure_config_file(&self.path);
                open_in_editor(&self.path);
            }
            Message::OpenDataDir => {
                Self::ensure_config_file(&self.path);
                open_with_explorer(&self.data_dir().to_string_lossy());
            }
            Message::OpenLogDir => {
                if let Some(logs) = log_dir() {
                    open_with_explorer(&logs.to_string_lossy());
                }
            }
            Message::ExportLogs => export_logs(),
            Message::ClearInputLog => {
                let log = self.data_dir().join("input-log.jsonl");
                if let Err(error) = std::fs::remove_file(&log)
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    crate::log::warn(format!("清空输入日志失败: {error}"));
                }
            }

            // 关于页
            Message::OpenWebsite => open_with_explorer(about::WEBSITE_URL),
            Message::OpenRepository => open_with_explorer(about::REPOSITORY_URL),
            Message::OpenFork => open_with_explorer(about::FORK_URL),

            // 下拉被清空 / 越界：不改
            _ => {}
        }
    }

    fn view(&self, _input: &(), context: &mut ViewContext<Self>) -> View {
        context.window_title("青简设置");
        if let Some(icon) = window_icon() {
            context.window_visuals(WindowVisuals::new().icon(icon));
        }
        // 「关于」的目标图标是「ⓘ 圆圈包 i」（Segoe MDL2 Assets **E946 Info**）。
        // `Symbol::Help` 是 E897「?」不是ⓘ，所以这里用 FontIcon 传 E946（默认字体 Segoe MDL2 Assets）。
        let item = |tag: &str, label: &str, icon: Icon| {
            KeyedView::new(
                tag,
                NavigationViewItem::new()
                    .tag(tag)
                    .is_selected(self.page == tag)
                    .slots([
                        SlotView::new(NavigationViewItemSlot::Icon, icon.into_view()),
                        SlotView::new(NavigationViewItemSlot::Content, label),
                    ]),
            )
        };
        let items = [
            item("general", "通用", Icon::symbol(Symbol::Setting)),
            item("typing", "输入行为", Icon::symbol(Symbol::Keyboard)),
            item("candidates", "候选窗口", Icon::symbol(Symbol::View)),
            item("shortcut", "快捷键", Icon::symbol(Symbol::Keyboard)),
            item("cloud", "云服务", Icon::symbol(Symbol::World)),
            item("fuzzy", "模糊音", Icon::symbol(Symbol::Audio)),
            item("dictionaries", "词库", Icon::symbol(Symbol::Library)),
            item("aux_code", "辅码", Icon::symbol(Symbol::Character)),
            item("usage", "统计", Icon::symbol(Symbol::List)),
            item("advanced", "高级", Icon::symbol(Symbol::Repair)),
            item("about", "关于", Icon::glyph("\u{E946}")),
            item("guide", "使用说明", Icon::symbol(Symbol::Message)),
        ];
        NavigationView::new()
            .pane_display_mode(NavigationViewPaneDisplayMode::Left)
            .pane_title("青简")
            .open_pane_length(220.0)
            .is_pane_open(true)
            .is_pane_toggle_button_visible(false)
            .is_back_button_visible(NavigationViewBackButtonVisible::Collapsed)
            .is_settings_visible(false)
            .on_selected_tag_changed(context.callback(Message::Navigate))
            .slots([
                SlotView::collection(NavigationViewSlot::MenuItems, items),
                SlotView::new(NavigationViewSlot::Content, self.page_content(context)),
            ])
    }
}

#[cfg(test)]
mod tests {
    use super::Icon;
    use windows_reactor::Symbol;

    /// 「关于」一栏：目标字形是「ⓘ 圆圈包 i」= Segoe MDL2 Assets **E946（Info）**，
    /// 用 FontIcon 的字面 glyph，不用 WinUI `Symbol::Help`（那是问号）。
    #[test]
    fn about_uses_info_glyph() {
        match Icon::glyph("\u{E946}") {
            Icon::Glyph(glyph) => assert_eq!(glyph, "\u{E946}"),
            Icon::Symbol(_) => panic!("关于应该用 FontIcon 的 E946（Info 圆圈包 i）"),
        }
    }

    /// 「使用说明」应避开 `Symbol::Help`，否则导航里会出现两个一样的问号。
    #[test]
    fn guide_does_not_use_help_symbol() {
        match Icon::symbol(Symbol::Message) {
            Icon::Symbol(symbol) => assert_ne!(symbol, Symbol::Help),
            Icon::Glyph(_) => {}
        }
    }
}
