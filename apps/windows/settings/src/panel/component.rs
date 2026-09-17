//! 根组件的 Reactor 生命周期：建状态、按消息落盘、画左侧导航 + 当前页。

use qingjian_platform::{
    CandidateRenderer, Config, DEFAULT_ENGLISH_CANDIDATES_OFF_WINDOWS, LayoutMode, LogLevel,
    PreeditMode, ShiftLetter, ThemeMode,
};
use windows_reactor::*;

use super::cloud_status::CloudStatus;
use super::controls::{export_logs, log_dir, open_in_editor, open_with_explorer};
use super::pages::{about, cloud, dictionaries, general, shortcut, typing};
use super::{Message, Settings};

/// 导航图标：要么是 Segoe MDL2 Assets 字形（`SymbolIcon`），要么是一个文字字符（`FontIcon`）。
///
/// 经验：极少数 Unicode 字符在本机字体里没有字形，会回退成「□」（豆腐框），比「?」还糟。
/// 一律用 Segoe MDL2 Assets 的 `Symbol::*` 字形做导航；个别 Symbol 在其它环境被回退成「?」
/// 也是显式字符，比豆腐框好。
enum Icon {
    Symbol(Symbol),
}

impl Icon {
    fn symbol(symbol: Symbol) -> Self {
        Self::Symbol(symbol)
    }

    fn into_view(self) -> View {
        match self {
            Self::Symbol(symbol) => SymbolIcon::new().symbol(symbol).into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Icon;
    use windows_reactor::Symbol;

    /// 「关于」一栏：Unicode `ℹ` 在某些 Windows 字体里没有字形，会被回退成「□」豆腐框，
    /// 所以统一走 Segoe MDL2 Assets 的 `Symbol::Help`（回退也只是「?」，不是豆腐框）。
    #[test]
    fn about_uses_help_symbol() {
        match Icon::symbol(Symbol::Help) {
            Icon::Symbol(symbol) => assert_eq!(symbol, Symbol::Help),
        }
    }

    /// 「使用说明」应避开 `Symbol::Help`，否则导航里会出现两个一样的图标。
    #[test]
    fn guide_does_not_use_help_symbol() {
        match Icon::symbol(Symbol::Message) {
            Icon::Symbol(symbol) => assert_ne!(symbol, Symbol::Help),
        }
    }
}

impl Component for Settings {
    type Input = ();
    type Message = Message;

    fn create(_input: &(), _context: &ComponentContext<Self>) -> Self {
        let path = Self::config_path();
        let config = Config::load(&path).unwrap_or_default();
        Self {
            config,
            path,
            page: "general".to_string(),
            cloud_status: CloudStatus::Idle,
            dictionary_status: String::new(),
            families: qingjian_render::system_fonts::families(),
            font_query: None,
        }
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::Navigate(Some(tag)) => self.page = tag,
            Message::Navigate(None) => {}

            // 通用页
            Message::LearningLanguage(Some(i)) if i < general::LANGUAGES.len() => {
                self.save("general", "learning_language", general::LANGUAGES[i].1);
            }
            Message::PageSize(Some(value)) => {
                let size = (value.round() as i64).clamp(1, 9);
                self.save("general", "page_size", size);
            }
            Message::Shuangpin(Some(i)) if i < general::SHUANGPIN.len() => {
                self.save("general", "shuangpin", general::SHUANGPIN[i].1);
            }
            Message::Zhuyin(on) => self.save("general", "zhuyin", on),
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

            // 候选窗口页
            Message::Theme(Some(i)) if i < ThemeMode::ALL.len() => {
                self.save("general", "theme", ThemeMode::ALL[i].key());
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

            // 高级页
            Message::VerboseLog(on) => {
                let level = if on { LogLevel::Debug } else { LogLevel::Info };
                self.save("general", "log_level", level.key());
            }
            Message::InputLog(on) => self.save("general", "input_log", on),
            Message::Learning(on) => self.save("general", "learning", on),
            Message::OpenConfigFile => open_in_editor(&self.path),
            Message::OpenDataDir => {
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
        // 导航图标全部走 Segoe MDL2 Assets（SymbolIcon）。Unity/U+ 字符在本机字体里可能没有字形
        // （会被回退成「□」豆腐框），Symbol 字形最差也只是回退成「?」，至少是显式字符。
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
            item("usage", "统计", Icon::symbol(Symbol::List)),
            item("advanced", "高级", Icon::symbol(Symbol::Repair)),
            item("about", "关于", Icon::symbol(Symbol::Help)),
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
