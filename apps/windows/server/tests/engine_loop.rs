//! 不经传输层，直接把协议消息喂给 Router 的闭环测试；用样例词库，跨平台可跑。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use qingjian_core::sentence::SentenceScorer;
use qingjian_core::{
    Language, ModeKeys, Prediction, PredictionPolicy, PredictionRequest, Predictor, ShuangpinScheme,
};
use qingjian_platform::protocol::{
    ClientMessage, Frame, InputSettings, KeyEvent, KeyModifiers, KeyOutcome, PROTOCOL_VERSION,
    ServerMessage, SessionId,
};
use qingjian_platform::{AppsConfig, DEFAULT_ENGLISH_CANDIDATES_OFF_WINDOWS, PreeditMode, Scheme};
use qingjian_windows_server::dispatch::{CandidateEvent, StatusEvent, StatusSink, StatusView};
use qingjian_windows_server::{AssemblySpec, Router, RouterConfig, assembly};

const SESSION: SessionId = SessionId(1);

/// Caps Lock 亮着。
const CAPS: KeyModifiers = KeyModifiers {
    ctrl: false,
    shift: false,
    alt: false,
    win: false,
    caps: true,
    english_mode: false,
};

/// 持久英文模式（Caps 灭）。
const ENGLISH: KeyModifiers = KeyModifiers {
    ctrl: false,
    shift: false,
    alt: false,
    win: false,
    caps: false,
    english_mode: true,
};

/// 样例词库装一个 Router，开好一个会话。
fn router() -> Router {
    router_with(RouterConfig::default())
}

fn router_with(config: RouterConfig) -> Router {
    router_in(config, None)
}

/// 在某个应用（宿主 exe 名）里开会话，名单用 Windows 缺省那份。
fn router_in_app(app: &str) -> Router {
    let config = RouterConfig {
        apps: AppsConfig::with_english_candidates_off(DEFAULT_ENGLISH_CANDIDATES_OFF_WINDOWS),
        ..RouterConfig::default()
    };
    router_in(config, Some(app.to_owned()))
}

/// `?` 开着当问字入口的 Router（配置 `[shortcut] question_mark`，缺省关）。
fn router_asking() -> Router {
    router_asking_with(RouterConfig::default())
}

fn router_asking_with(config: RouterConfig) -> Router {
    let mut router = router_with(config);
    router.engine_mut().set_mode_keys(ModeKeys {
        question_mark: true,
        ..ModeKeys::default()
    });
    router
}

fn router_in(config: RouterConfig, app: Option<String>) -> Router {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let dict = root.join("assets/sample/dict.tsv");
    let glossary = root.join("assets/sample/glossary-en.tsv");
    let mut engine = assembly::assemble(&AssemblySpec {
        glossary: Some((Language::English, glossary)),
        english: Some(root.join("assets/sample/english.tsv")),
        ..AssemblySpec::new(dict)
    })
    .expect("assemble engine from sample data");
    // 与 main.rs 一样，双拼方案是启动时直接设给 Engine 的。
    engine.set_shuangpin(config.scheme.shuangpin());
    let mut router = Router::new(engine, config);
    // 协议版本与 Server 一致：开会话时把按键行为设置回一次（DLL 不读配置文件，靠它拿切换键）。
    open_session(&mut router, SESSION, app);
    router
}

/// 会话的宿主 `(进程 id, 线程 id)`：按会话号编一个，够互相区分就行。
/// Server 认「前台窗口属于哪个会话」靠的就是这两个数（见 `ui::foreground`）。
fn host_of(session: SessionId) -> (u32, u32) {
    (1000 + session.0 as u32 * 100, session.0 as u32)
}

/// 开一个会话并吃掉 Server 回的按键行为设置。
fn open_session(router: &mut Router, session: SessionId, app: Option<String>) {
    let (pid, tid) = host_of(session);
    match router.handle(ClientMessage::OpenSession {
        session,
        app,
        pid,
        tid,
        protocol: PROTOCOL_VERSION,
    }) {
        Some(ServerMessage::SessionOpened { .. }) => {}
        other => panic!("expected SessionOpened, got {other:?}"),
    }
}

/// 让 Router 以为「前台窗口是 `session` 的宿主」：报它的线程与进程，与真钩子报的线索同形。
fn focus_window(router: &mut Router, session: SessionId) {
    let (pid, tid) = host_of(session);
    router.handle_foreground(vec![(tid, pid)]);
}

fn letter(c: char) -> KeyEvent {
    letter_with(c, Default::default())
}

/// `c` 的大小写就是 DLL 按 Shift 解析出的字符。
fn letter_with(c: char, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(c.to_ascii_uppercase() as u32, Some(c), modifiers)
}

fn press(router: &mut Router, event: KeyEvent) -> (KeyOutcome, Option<String>, Frame) {
    key_result(router.handle(ClientMessage::Key {
        session: SESSION,
        event,
    }))
}

/// 英文模式下敲一串字母，返回最后一次的处理结果。
fn type_english(router: &mut Router, text: &str) -> (KeyOutcome, Option<String>, Frame) {
    let mut last = None;
    for c in text.chars() {
        last = Some(press(router, letter_with(c, ENGLISH)));
    }
    last.expect("typed at least one letter")
}

fn candidate_texts(frame: &Frame) -> Vec<&str> {
    frame
        .candidates
        .items
        .iter()
        .map(|c| c.text.as_str())
        .collect()
}

fn digit(n: u32) -> KeyEvent {
    digit_with(n, Default::default())
}

/// 数字键 1–9；`character` 按 DLL 的解析：按着 Shift 是上档字符。
fn digit_with(n: u32, modifiers: KeyModifiers) -> KeyEvent {
    let c = if modifiers.shift {
        b")!@#$%^&*("[n as usize] as char
    } else {
        char::from_digit(n, 10).unwrap()
    };
    KeyEvent::new(0x30 + n, Some(c), modifiers)
}

const SHIFT: KeyModifiers = KeyModifiers {
    shift: true,
    ..ALT_OFF
};

const CTRL: KeyModifiers = KeyModifiers {
    ctrl: true,
    ..ALT_OFF
};

const ALT_OFF: KeyModifiers = KeyModifiers {
    ctrl: false,
    shift: false,
    alt: false,
    win: false,
    caps: false,
    english_mode: false,
};

/// 两个平台的缺省快捷键都没用 Win，拿来测「没配到的修饰键归应用」。
const WIN: KeyModifiers = KeyModifiers {
    win: true,
    ..ALT_OFF
};

/// 平台缺省的译词键：macOS 是 Alt，Windows 是 Ctrl（Alt 被系统菜单截走）。
#[cfg(not(windows))]
const TRANSLATE: KeyModifiers = KeyModifiers {
    alt: true,
    ..ALT_OFF
};
#[cfg(windows)]
const TRANSLATE: KeyModifiers = KeyModifiers {
    ctrl: true,
    ..ALT_OFF
};

const TRANSLATE_SECOND: KeyModifiers = KeyModifiers {
    shift: true,
    ..TRANSLATE
};

/// 当前页里 `text` 排第几（1 起）。
fn slot_of(frame: &Frame, text: &str) -> u32 {
    let position = candidate_texts(frame)
        .iter()
        .position(|t| *t == text)
        .unwrap_or_else(|| panic!("{text} 应在当前页：{:?}", candidate_texts(frame)));
    position as u32 + 1
}

/// 带字符的按键（标点等），虚拟键码随便给一个 OEM 键。
fn punct(c: char) -> KeyEvent {
    KeyEvent::new(0xBE, Some(c), Default::default())
}

fn key_result(message: Option<ServerMessage>) -> (KeyOutcome, Option<String>, Frame) {
    match message {
        Some(ServerMessage::KeyResult {
            outcome,
            commit,
            frame,
            ..
        }) => (outcome, commit, frame),
        other => panic!("expected KeyResult, got {other:?}"),
    }
}

/// 中文模式下敲一串字母，返回最后一次的处理结果。
fn type_letters(router: &mut Router, text: &str) -> (KeyOutcome, Option<String>, Frame) {
    let mut last = None;
    for c in text.chars() {
        last = Some(key_result(router.handle(ClientMessage::Key {
            session: SESSION,
            event: letter(c),
        })));
    }
    last.expect("typed at least one letter")
}

fn preedit(frame: &Frame) -> String {
    frame.preedit.iter().map(|s| s.text.as_str()).collect()
}

#[test]
fn typing_pinyin_shows_candidates() {
    let mut router = router();
    let (outcome, commit, frame) = type_letters(&mut router, "nihao");

    assert_eq!(outcome, KeyOutcome::Consumed);
    assert_eq!(commit, None);
    assert_eq!(preedit(&frame), "ni'hao");
    let texts: Vec<&str> = frame
        .candidates
        .items
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert!(
        texts.contains(&"你好"),
        "候选里应有「你好」，实际：{texts:?}"
    );
}

/// 拼音显示位置随帧下发给 DLL：DLL 按 `inline()` 决定要不要往应用里放行内拼音，
/// 窗口顶部画不画拼音行由 Server 自己按 `in_window()` 定，所以帧始终带着拼音行。
#[test]
fn frame_carries_the_preedit_mode() {
    for mode in PreeditMode::ALL {
        let mut router = router_with(RouterConfig {
            preedit: mode,
            ..RouterConfig::default()
        });
        let (_, _, frame) = type_letters(&mut router, "nihao");

        assert_eq!(frame.preedit_mode, mode);
        assert_eq!(
            preedit(&frame),
            "ni'hao",
            "{mode:?} 下帧也要带拼音行，画不画是窗口的事"
        );
    }
}

#[test]
fn selecting_by_digit_commits_and_clears() {
    let mut router = router();
    let (_, _, frame) = type_letters(&mut router, "nihao");
    let position = frame
        .candidates
        .items
        .iter()
        .position(|c| c.text == "你好")
        .expect("「你好」在候选页内");
    let (outcome, commit, after) = key_result(router.handle(ClientMessage::Key {
        session: SESSION,
        event: digit(position as u32 + 1),
    }));

    assert_eq!(outcome, KeyOutcome::Consumed);
    assert_eq!(commit.as_deref(), Some("你好"));
    assert!(
        after.is_empty(),
        "上屏后应收起候选，实际 preedit={:?}",
        preedit(&after)
    );
}

#[test]
fn space_commits_first_candidate() {
    let mut router = router();
    type_letters(&mut router, "ni");
    let space = KeyEvent::new(0x20, Some(' '), Default::default());
    let (outcome, commit, after) = key_result(router.handle(ClientMessage::Key {
        session: SESSION,
        event: space,
    }));

    assert_eq!(outcome, KeyOutcome::Consumed);
    assert_eq!(commit.as_deref(), Some("你"), "「ni」首选应是「你」");
    assert!(after.is_empty());
}

#[test]
fn backspace_shrinks_preedit() {
    let mut router = router();
    let (_, _, frame) = type_letters(&mut router, "nihao");
    assert_eq!(preedit(&frame), "ni'hao");
    let back = KeyEvent::new(0x08, None, Default::default());
    let (outcome, _, after) = key_result(router.handle(ClientMessage::Key {
        session: SESSION,
        event: back,
    }));

    assert_eq!(outcome, KeyOutcome::Consumed);
    assert_eq!(preedit(&after), "ni'ha");
}

#[test]
fn non_letter_without_composing_passes_through() {
    let mut router = router();
    let space = KeyEvent::new(0x20, Some(' '), Default::default());
    let (outcome, commit, frame) = key_result(router.handle(ClientMessage::Key {
        session: SESSION,
        event: space,
    }));

    assert_eq!(outcome, KeyOutcome::Passthrough);
    assert_eq!(commit, None);
    assert!(frame.is_empty());
}

#[test]
fn focus_leave_commits_raw_pinyin() {
    let mut router = router();
    type_letters(&mut router, "nihao");
    let committed = router.handle(ClientMessage::Commit { session: SESSION });
    assert_eq!(
        committed,
        Some(ServerMessage::Committed {
            session: SESSION,
            text: Some("nihao".to_owned()),
        })
    );
    let (_, _, frame) = type_letters(&mut router, "ni");
    assert_eq!(preedit(&frame), "ni");
    // 没在组句时 Commit 不交东西。
    router.handle(ClientMessage::Key {
        session: SESSION,
        event: KeyEvent::new(0x1B, None, Default::default()),
    });
    assert_eq!(
        router.handle(ClientMessage::Commit { session: SESSION }),
        Some(ServerMessage::Committed {
            session: SESSION,
            text: None,
        })
    );
}

#[test]
fn commit_from_other_session_does_not_take_buffer() {
    let mut router = router();
    type_letters(&mut router, "ni");
    let other = SessionId(2);
    open_session(&mut router, other, None);
    // 别的会话拿不到这个会话的拼音，但残留组句一并清掉。
    assert_eq!(
        router.handle(ClientMessage::Commit { session: other }),
        Some(ServerMessage::Committed {
            session: other,
            text: None,
        })
    );
    let space = KeyEvent::new(0x20, Some(' '), Default::default());
    let (outcome, _, _) = key_result(router.handle(ClientMessage::Key {
        session: SESSION,
        event: space,
    }));
    assert_eq!(outcome, KeyOutcome::Passthrough);
}

#[test]
fn page_keys_follow_config() {
    // 每页 1 条保证多页；翻页键改成 `,` `.`。
    let mut router = router_with(RouterConfig {
        page_size: 1,
        page_keys: (',', '.'),
        ..RouterConfig::default()
    });
    let (_, _, frame) = type_letters(&mut router, "ni");
    assert!(frame.page_count > 1, "样例词库里 ni 应不止一个候选");
    assert_eq!(frame.page, 0);

    let key = |router: &mut Router, c| {
        key_result(router.handle(ClientMessage::Key {
            session: SESSION,
            event: punct(c),
        }))
    };
    let (outcome, commit, frame) = key(&mut router, '.');
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert_eq!(frame.page, 1, "`.` 应翻到下一页");
    let (_, _, frame) = key(&mut router, ',');
    assert_eq!(frame.page, 0, "`,` 应翻回上一页");
    // 缺省的 `]` 此时不再翻页，进直输段。
    let (_, _, frame) = key(&mut router, ']');
    assert_eq!(frame.page, 0);
    assert!(
        preedit(&frame).contains(']'),
        "`]` 应进直输段：{}",
        preedit(&frame)
    );
}

#[test]
fn minus_equals_page_keys_preserve_expression_input() {
    let mut router = router_with(RouterConfig {
        page_size: 1,
        page_keys: ('-', '='),
        ..RouterConfig::default()
    });
    let (_, _, frame) = type_letters(&mut router, "ni");
    assert!(frame.page_count > 1);
    let (outcome, commit, frame) = press(&mut router, punct('='));
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert_eq!(frame.page, 1);
    assert_eq!(preedit(&frame), "ni");
    let (_, _, frame) = press(&mut router, punct('-'));
    assert_eq!(frame.page, 0);
    assert_eq!(preedit(&frame), "ni");
    press(&mut router, KeyEvent::new(0x1B, None, Default::default()));

    type_letters(&mut router, "v");
    for c in "2-1=".chars() {
        let (outcome, commit, _) = press(&mut router, punct(c));
        assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    }
    let (outcome, commit, _) = press(&mut router, punct(' '));
    assert_eq!(
        (outcome, commit),
        (KeyOutcome::Consumed, Some("2-1=1".to_owned()))
    );
}

#[test]
fn english_mode_gives_candidates_and_space_picks_highlighted() {
    let mut router = router();
    let (outcome, commit, frame) = type_english(&mut router, "hel");
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert_eq!(preedit(&frame), "hel", "英文模式敲的字母原样显示");
    let texts = candidate_texts(&frame);
    assert!(
        texts.contains(&"hello") && texts.contains(&"help"),
        "候选应来自英文词表：{texts:?}"
    );
    // 空格与中文模式一样选高亮的词，词后接上空格（放行会让应用先插空格）。
    let first = frame.candidates.items[0].text.clone();
    let (outcome, commit, after) = press(&mut router, KeyEvent::new(0x20, Some(' '), ENGLISH));
    assert_eq!(outcome, KeyOutcome::Consumed);
    assert_eq!(commit, Some(format!("{first} ")));
    assert!(after.is_empty());

    // 回车仍把字母原样上屏：词表没有的写法靠它。
    type_english(&mut router, "hel");
    let (outcome, commit, _) = press(&mut router, function_key(0x0D));
    assert_eq!(
        (outcome, commit.as_deref()),
        (KeyOutcome::Consumed, Some("hel"))
    );
}

#[test]
fn english_digits_pick_candidates_or_join_the_word() {
    let mut router = router();
    // 有候选：数字选当前页第 N 个，与中文模式一样。
    let (_, _, frame) = type_english(&mut router, "hel");
    let second = frame.candidates.items[1].text.clone();
    let (outcome, commit, after) = press(&mut router, KeyEvent::new(0x32, Some('2'), ENGLISH));
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, Some(second)));
    assert!(after.is_empty());

    // 没候选（词表没有的词）：数字是标识符的一部分，回车整段原样上屏。
    let (_, _, frame) = type_english(&mut router, "xq");
    assert!(
        frame.candidates.items.is_empty(),
        "样例词表里没有 xq 开头的词"
    );
    let (outcome, commit, frame) = press(&mut router, KeyEvent::new(0x31, Some('1'), ENGLISH));
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert_eq!(preedit(&frame), "xq1");
    let (_, commit, _) = press(&mut router, function_key(0x0D));
    assert_eq!(commit.as_deref(), Some("xq1"));
}

#[test]
fn caps_lock_types_direct_uppercase_english_regardless_of_mode() {
    let mut router = router();
    let (outcome, commit, frame) = press(&mut router, letter_with('H', CAPS));
    assert_eq!(
        (outcome, commit.as_deref()),
        (KeyOutcome::Consumed, Some("H"))
    );
    assert!(frame.is_empty(), "Caps 直接上屏不出候选：{frame:?}");
    // 组句中 Caps 亮着敲字母：拼音先原样上屏，再接大写字母。
    type_letters(&mut router, "ni");
    let (_, commit, after) = press(&mut router, letter_with('A', CAPS));
    assert_eq!(commit.as_deref(), Some("niA"));
    assert!(after.is_empty());
}

#[test]
fn english_candidates_are_off_in_listed_apps_by_exe_name() {
    // VS Code 在缺省名单里（exe 名不区分大小写）：英文模式字母直插、不出候选。
    let mut router = router_in_app("code.exe");
    let (outcome, commit, frame) = press(&mut router, letter_with('h', ENGLISH));
    assert_eq!(
        (outcome, commit.as_deref()),
        (KeyOutcome::Consumed, Some("h"))
    );
    assert!(frame.is_empty(), "名单里的应用不该有候选：{frame:?}");
    let (outcome, commit, _) = press(&mut router, KeyEvent::new(0x20, Some(' '), ENGLISH));
    assert_eq!((outcome, commit), (KeyOutcome::Passthrough, None));
    // 中文模式不受名单影响。
    let (_, _, frame) = type_letters(&mut router, "ni");
    assert!(!frame.candidates.items.is_empty(), "拼音照常出候选");
}

#[test]
fn english_candidates_stay_on_in_other_apps() {
    let mut router = router_in_app("notepad.exe");
    let (outcome, commit, frame) = type_english(&mut router, "hel");
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert!(
        candidate_texts(&frame).contains(&"hello"),
        "不在名单里的应用照常给英文候选：{frame:?}"
    );
}

#[test]
fn app_list_is_looked_up_per_session() {
    // 两个应用同时在线：切会话时按各自的 exe 名判断。
    let mut router = router_in_app("Code.exe");
    let notepad = SessionId(2);
    open_session(&mut router, notepad, Some("notepad.exe".to_owned()));
    let (_, _, frame) = key_result(router.handle(ClientMessage::Key {
        session: notepad,
        event: letter_with('h', ENGLISH),
    }));
    assert_eq!(preedit(&frame), "h", "记事本会话组词");
    // 切回编辑器会话：残留组句清掉，字母直插。
    let (outcome, commit, after) = press(&mut router, letter_with('e', ENGLISH));
    assert_eq!(
        (outcome, commit.as_deref()),
        (KeyOutcome::Consumed, Some("e"))
    );
    assert!(after.is_empty());
}

#[test]
fn english_tab_and_arrow_keys_pick_candidates() {
    let mut router = router();
    let (_, _, frame) = type_english(&mut router, "hel");
    let first = frame.candidates.items[0].text.clone();
    // Tab 选高亮的词。
    let (outcome, commit, _) = press(&mut router, KeyEvent::new(0x09, None, ENGLISH));
    assert_eq!(
        (outcome, commit.as_deref()),
        (KeyOutcome::Consumed, Some(first.as_str()))
    );

    // 方向键移到第二个之后，空格选的是它，再接上空格。
    let (_, _, frame) = type_english(&mut router, "hel");
    let second = frame.candidates.items[1].text.clone();
    let (outcome, _, _) = press(&mut router, KeyEvent::new(0x28, None, ENGLISH));
    assert_eq!(outcome, KeyOutcome::Consumed);
    let (_, commit, after) = press(&mut router, KeyEvent::new(0x20, Some(' '), ENGLISH));
    assert_eq!(commit, Some(format!("{second} ")));
    assert!(after.is_empty());
}

#[test]
fn english_without_candidates_is_passthrough_with_shift_case() {
    let mut router = router_with(RouterConfig {
        english_candidates: false,
        ..RouterConfig::default()
    });
    // 字母由我们插入，大小写按 Shift；不组句。
    let (outcome, commit, frame) = press(&mut router, letter_with('h', ENGLISH));
    assert_eq!(
        (outcome, commit.as_deref()),
        (KeyOutcome::Consumed, Some("h"))
    );
    assert!(frame.is_empty());
    let shifted = KeyModifiers {
        shift: true,
        ..ENGLISH
    };
    let (_, commit, _) = press(&mut router, letter_with('H', shifted));
    assert_eq!(commit.as_deref(), Some("H"));
    // 其他键交给应用。
    let (outcome, commit, _) = press(&mut router, KeyEvent::new(0x20, Some(' '), ENGLISH));
    assert_eq!((outcome, commit), (KeyOutcome::Passthrough, None));
}

#[test]
fn switching_to_chinese_mid_word_flushes_english_letters() {
    let mut router = router();
    type_english(&mut router, "hel");
    // 切回中文模式再敲字母：之前的英文字母原样上屏，新字母从头当拼音。
    let (outcome, commit, frame) = press(&mut router, letter('l'));
    assert_eq!(
        (outcome, commit.as_deref()),
        (KeyOutcome::Consumed, Some("hel"))
    );
    assert_eq!(preedit(&frame), "l");
}

#[test]
fn shift_uppercase_while_composing_goes_into_the_buffer_when_configured() {
    // 配成 `shift_letter = "compose"` 才有这条：缺省是交给应用（见 `shift_letters_follow_the_configuration`）。
    let config = RouterConfig {
        shift_letter_compose: true,
        ..RouterConfig::default()
    };
    let mut router = router_with(config);
    type_letters(&mut router, "ni");
    // 中文模式按住 Shift 打大写字母：进缓冲区（不再直接交给应用），拼音行照敲的样子显示。
    let shifted = KeyModifiers {
        shift: true,
        ..KeyModifiers::default()
    };
    let (outcome, commit, frame) = press(&mut router, letter_with('A', shifted));
    assert_eq!((outcome, commit.as_deref()), (KeyOutcome::Consumed, None));
    assert_eq!(preedit(&frame), "niA");
    // 回车原样上屏，大写还原。
    let (_, commit, _) = press(&mut router, function_key(0x0D));
    assert_eq!(commit.as_deref(), Some("niA"));
}

#[test]
fn alt_digit_commits_first_translation() {
    let mut router = router();
    let (_, _, frame) = type_letters(&mut router, "nihao");
    let slot = slot_of(&frame, "你好");
    // 缺省译词键（mac ⌥ / Windows Ctrl）+ 数字：上屏那个候选的第一个译词，组句结束。
    let (outcome, commit, after) = press(&mut router, digit_with(slot, TRANSLATE));
    assert_eq!(
        (outcome, commit.as_deref()),
        (KeyOutcome::Consumed, Some("hello"))
    );
    assert!(after.is_empty());
}

#[test]
fn second_translation_key_without_second_sense_is_swallowed() {
    let mut router = router();
    let (_, _, frame) = type_letters(&mut router, "nihao");
    let slot = slot_of(&frame, "你好");
    // 样例释义表里「你好」只有一条译文：第二个译词键（Shift+译词键）+ 数字吞掉不动，组句还在。
    let (outcome, commit, after) = press(&mut router, digit_with(slot, TRANSLATE_SECOND));
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert_eq!(preedit(&after), "ni'hao");
}

#[test]
fn shift_digit_forgets_candidate_and_requeries() {
    let mut router = router();
    let (_, _, frame) = type_letters(&mut router, "nihao");
    let slot = slot_of(&frame, "你好");
    // 缺省 Shift + 数字：删候选（词库词只清学习记录），重新查一遍，组句不变。
    let (outcome, commit, after) = press(&mut router, digit_with(slot, SHIFT));
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert_eq!(preedit(&after), "ni'hao");
    assert!(!after.candidates.items.is_empty());
}

#[test]
fn unconfigured_modifier_digit_is_not_a_selection() {
    // 删候选改成 Ctrl+Shift：Shift+4 就是普通的 `$`；Win+1 没配到快捷键，归应用。
    let mut router = router_with(RouterConfig {
        delete_keys: KeyModifiers {
            shift: true,
            ..CTRL
        },
        ..RouterConfig::default()
    });
    type_letters(&mut router, "nihao");
    let (outcome, commit, frame) = press(&mut router, digit_with(4, SHIFT));
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert!(preedit(&frame).contains('$'), "{}", preedit(&frame));
    let (outcome, commit, _) = press(&mut router, digit_with(1, WIN));
    assert_eq!((outcome, commit), (KeyOutcome::Passthrough, None));
}

#[test]
fn learning_data_persists_to_user_dir() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let user_dir =
        std::env::temp_dir().join(format!("qingjian-windows-learning-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&user_dir);
    std::fs::create_dir_all(&user_dir).unwrap();
    let engine = assembly::assemble(&AssemblySpec {
        glossary: Some((
            Language::English,
            root.join("assets/sample/glossary-en.tsv"),
        )),
        user_dir: Some(user_dir.clone()),
        ..AssemblySpec::new(root.join("assets/sample/dict.tsv"))
    })
    .unwrap();
    let mut router = Router::new(engine, RouterConfig::default());
    open_session(&mut router, SESSION, None);
    let (_, _, frame) = type_letters(&mut router, "nihao");
    let position = frame
        .candidates
        .items
        .iter()
        .position(|c| c.text == "你好")
        .unwrap();
    router.handle(ClientMessage::Key {
        session: SESSION,
        event: digit(position as u32 + 1),
    });
    // 关会话时落盘。
    router.handle(ClientMessage::CloseSession { session: SESSION });

    let user = std::fs::read_to_string(user_dir.join("user.tsv")).expect("user.tsv 应已写出");
    assert!(user.contains("你好"), "user.tsv 里应记了「你好」：{user}");
    assert!(user_dir.join("usage.tsv").is_file(), "usage.tsv 应已写出");
    let _ = std::fs::remove_dir_all(&user_dir);
}

/// 记录状态条调用：`Some(模式格文字)` 是显示、`None` 是收起。
#[derive(Clone, Default)]
struct RecordingStatus(Arc<Mutex<Vec<Option<String>>>>);

impl RecordingStatus {
    fn calls(&self) -> Vec<Option<String>> {
        self.0.lock().unwrap().clone()
    }
}

impl StatusSink for RecordingStatus {
    fn show_status(&self, view: StatusView) {
        // 状态条模式格的实际写法（`StatusView::mode_text`，与 Server 画的是同一份实现）。
        self.0.lock().unwrap().push(Some(view.mode_text()));
    }

    fn hide_status(&self) {
        self.0.lock().unwrap().push(None);
    }
}

/// 开着状态条、接好记录器的 Router（已开好 [`SESSION`] 那个会话）。
fn status_router() -> (Router, RecordingStatus) {
    let mut router = router_with(RouterConfig {
        status_enabled: true,
        ..RouterConfig::default()
    });
    let recorder = RecordingStatus::default();
    router.set_status_sink(Box::new(recorder.clone()));
    (router, recorder)
}

/// 某会话报来它的中英模式（DLL 在激活与切模式时发）。
fn mode_changed(router: &mut Router, session: SessionId, english: bool) {
    mode_changed_with(router, session, english, false);
}

/// 连 Caps Lock 一起报：状态条的模式格前面会多一个「A」。
fn mode_changed_with(router: &mut Router, session: SessionId, english: bool, caps: bool) {
    router.handle(ClientMessage::ModeChanged {
        session,
        english,
        caps,
    });
}

#[test]
fn status_bar_mode_click_is_handed_to_dll_via_sync_mode() {
    let config = RouterConfig {
        status_enabled: true,
        ..RouterConfig::default()
    };
    let mut router = router_with(config);
    let recorder = RecordingStatus::default();
    router.set_status_sink(Box::new(recorder.clone()));
    router.handle(ClientMessage::ModeChanged {
        session: SESSION,
        english: false,
        caps: false,
    });

    // 点「中」：状态条先翻成「英」，DLL 来取时拿到目标模式，取一次就清。
    router.handle_status_event(StatusEvent::ToggleMode);
    assert_eq!(recorder.calls().last(), Some(&Some("英".to_owned())));
    assert_eq!(
        router.handle(ClientMessage::SyncMode { session: SESSION }),
        Some(ServerMessage::ModeSync {
            session: SESSION,
            english: Some(true),
            input: InputSettings::default(),
        })
    );
    assert_eq!(
        router.handle(ClientMessage::SyncMode { session: SESSION }),
        Some(ServerMessage::ModeSync {
            session: SESSION,
            english: None,
            input: InputSettings::default(),
        })
    );
}

#[test]
fn status_bar_mode_click_is_ignored_when_builtin_english_is_off() {
    let config = RouterConfig {
        status_enabled: true,
        english_mode: false,
        ..RouterConfig::default()
    };
    let mut router = router_with(config);
    let recorder = RecordingStatus::default();
    router.set_status_sink(Box::new(recorder.clone()));
    router.handle(ClientMessage::ModeChanged {
        session: SESSION,
        english: false,
        caps: false,
    });
    assert_eq!(recorder.calls().last(), Some(&Some("拼".to_owned())));

    // 关掉内置英文模式：点「中」不翻成「英」，也不给 DLL 递目标模式（DLL 那边同样会拦）
    router.handle_status_event(StatusEvent::ToggleMode);
    assert_eq!(recorder.calls().last(), Some(&Some("拼".to_owned())));
    assert_eq!(
        router.handle(ClientMessage::SyncMode { session: SESSION }),
        Some(ServerMessage::ModeSync {
            session: SESSION,
            english: None,
            input: InputSettings {
                english_mode: false,
                ..InputSettings::default()
            },
        })
    );
}

#[test]
fn chinese_punctuation_is_full_width_only_when_not_composing() {
    let mut router = router();
    // 没在组句：逗号转全角；数字后的点保持半角。
    let comma = KeyEvent::new(0xBC, Some(','), Default::default());
    assert_eq!(
        press(&mut router, comma),
        (
            KeyOutcome::Consumed,
            Some("，".to_owned()),
            Frame::default()
        )
    );
    press(&mut router, digit(3));
    let period = KeyEvent::new(0xBE, Some('.'), Default::default());
    assert_eq!(press(&mut router, period).0, KeyOutcome::Passthrough);
    assert_eq!(press(&mut router, period).1, Some("。".to_owned()));

    // 小键盘的点不跟在数字后面也保持半角（#150 把这些键收进输入法之后）。
    let keypad_period = KeyEvent::new(0x6E, Some('.'), Default::default());
    assert_eq!(press(&mut router, keypad_period).0, KeyOutcome::Passthrough);

    // 组句中：标点进英文直输段，不转。
    type_letters(&mut router, "ni");
    let (outcome, commit, frame) = press(&mut router, comma);
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert!(!frame.is_empty(), "组句应还在");

    // 状态条上关掉全角：原样交给应用。
    router.handle(ClientMessage::Commit { session: SESSION });
    router.handle_status_event(StatusEvent::TogglePunctuation);
    assert_eq!(press(&mut router, comma).0, KeyOutcome::Passthrough);
}

#[test]
fn hyphen_and_equals_are_inserted_by_us_instead_of_passed_through() {
    let mut router = router();
    // `-` `=` 没有全角映射，但由我们插入：放行那条路在部分宿主里到不了应用（中文模式按 - 没反应）
    let hyphen = KeyEvent::new(0xBD, Some('-'), Default::default());
    assert_eq!(
        press(&mut router, hyphen),
        (KeyOutcome::Consumed, Some("-".to_owned()), Frame::default())
    );
    let equals = KeyEvent::new(0xBB, Some('='), Default::default());
    assert_eq!(
        press(&mut router, equals),
        (KeyOutcome::Consumed, Some("=".to_owned()), Frame::default())
    );
    // 其他没有全角映射的键（`@`）仍原样交给应用
    let at = KeyEvent::new(0x32, Some('@'), SHIFT);
    assert_eq!(press(&mut router, at).0, KeyOutcome::Passthrough);
    // 组句中的 `-` 仍进英文直输段，不插字符
    type_letters(&mut router, "ni");
    let (outcome, commit, frame) = press(&mut router, hyphen);
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert!(
        preedit(&frame).contains('-'),
        "应进直输段: {:?}",
        preedit(&frame)
    );
}

#[test]
fn raw_segment_takes_digits_and_keeps_the_space() {
    // `-` 进英文直输段之后数字是内容不是选词键，空格整段原样上屏并保留空格（`gpt-6` 曾经丢了 6）。
    let mut router = router();
    let mut frame = Frame::default();
    for c in "gpt-6".chars() {
        let (outcome, commit, next) = press(&mut router, letter(c));
        assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
        frame = next;
    }
    assert_eq!(preedit(&frame), "gpt-6");
    let (outcome, commit, after) = press(&mut router, letter(' '));
    assert_eq!(
        (outcome, commit.as_deref()),
        (KeyOutcome::Consumed, Some("gpt-6 "))
    );
    assert!(after.is_empty());
}

#[test]
fn digit_without_a_slot_joins_the_buffer() {
    // 这一页没有第 9 格：数字是内容（`gpt9`），不再被静默吞掉；成了直输段之后空格整段上屏。
    let mut router = router();
    let (_, _, frame) = type_letters(&mut router, "gpt");
    let shown = frame.candidates.items.len();
    assert!(
        (1..9).contains(&shown),
        "样例词库下 gpt 的候选应不满 9 个：{shown}"
    );
    let (outcome, commit, frame) = press(&mut router, digit(9));
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert_eq!(preedit(&frame), "gpt9");
    let (_, commit, after) = press(&mut router, letter(' '));
    assert_eq!(commit.as_deref(), Some("gpt9 "));
    assert!(after.is_empty());
    // 有这一格照常选词。
    let (_, _, frame) = type_letters(&mut router, "ni");
    let first = frame.candidates.items[0].text.clone();
    let (_, commit, _) = press(&mut router, digit(1));
    assert_eq!(commit, Some(first));
}

#[test]
fn english_digit_without_a_slot_joins_the_word() {
    let mut router = router();
    let (_, _, frame) = type_english(&mut router, "hello");
    let shown = frame.candidates.items.len();
    assert!((1..9).contains(&shown), "hello 的候选应不满 9 个：{shown}");
    let (outcome, commit, frame) = press(&mut router, digit_with(9, ENGLISH));
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert_eq!(preedit(&frame), "hello9");
}

#[test]
fn status_bar_follows_mode_when_enabled() {
    let (mut router, recorder) = status_router();

    // 中文 → 英文：各刷一次。
    mode_changed(&mut router, SESSION, false);
    mode_changed(&mut router, SESSION, true);
    // 前台会话关了（应用退出）：状态条失去依据，收起 —— 接着显示一个已经不存在的会话的模式就是骗人。
    router.handle(ClientMessage::CloseSession { session: SESSION });
    assert_eq!(
        recorder.calls(),
        vec![Some("拼".to_owned()), Some("英".to_owned()), None]
    );
}

#[test]
fn status_bar_hides_when_the_foreground_session_switches_to_another_ime() {
    let (mut router, recorder) = status_router();

    mode_changed(&mut router, SESSION, false);
    assert_eq!(recorder.calls().last(), Some(&Some("拼".to_owned())));

    router.handle(ClientMessage::ImeSwitched { session: SESSION });
    assert_eq!(recorder.calls().last(), Some(&None));
}

/// 中英模式按会话各记一份，状态条只显示前台那一份：切应用时跟着变，后台应用报来的带不跑它。
#[test]
fn status_bar_follows_the_foreground_app() {
    let (mut router, recorder) = status_router();
    let notepad = SessionId(2);
    open_session(&mut router, notepad, Some("notepad.exe".to_owned()));

    // 编辑器中文、记事本英文。还没有前台线索时报模式的这个先当上前台，所以显示编辑器的「中」。
    mode_changed(&mut router, SESSION, false);
    mode_changed(&mut router, notepad, true);
    assert_eq!(recorder.calls().last(), Some(&Some("拼".to_owned())));

    // 切到记事本：翻成「英」——记事本没再报过模式，Server 记着它激活时那一份。
    focus_window(&mut router, notepad);
    assert_eq!(recorder.calls().last(), Some(&Some("英".to_owned())));

    // 切回来还是「中」；这时后台的记事本报模式（配置改了之类）也不许把状态条带跑。
    focus_window(&mut router, SESSION);
    assert_eq!(recorder.calls().last(), Some(&Some("拼".to_owned())));
    mode_changed(&mut router, notepad, false);
    assert_eq!(recorder.calls().last(), Some(&Some("拼".to_owned())));
}

/// 前台窗口不属于任何会话（那个应用没装青简 / 用的是别的输入法）：收起，别接着显示上一个应用的模式。
#[test]
fn status_bar_hides_when_the_foreground_window_has_no_session() {
    let (mut router, recorder) = status_router();

    mode_changed(&mut router, SESSION, false);
    assert_eq!(recorder.calls().last(), Some(&Some("拼".to_owned())));

    router.handle_foreground(vec![(9999, 8888)]);
    assert_eq!(recorder.calls().last(), Some(&None));
}

/// 商店应用的前台窗口是 `ApplicationFrameHost` 的框架窗，线程号对不上任何会话；
/// 真正装了青简的窗口在它的后代里，退一步按进程 id 也能认出来。
#[test]
fn foreground_window_falls_back_to_the_process_id() {
    let (mut router, recorder) = status_router();

    mode_changed(&mut router, SESSION, false);
    router.handle_foreground(vec![(9999, 8888)]);
    assert_eq!(recorder.calls().last(), Some(&None));

    let (pid, _) = host_of(SESSION);
    router.handle_foreground(vec![(9999, 8888), (7, pid)]);
    assert_eq!(recorder.calls().last(), Some(&Some("拼".to_owned())));
}

/// 线程号会被系统回收：只有进程号也对得上才算同一个会话，否则会认到早已退出的那个。
#[test]
fn foreground_match_needs_both_the_thread_and_the_process() {
    let (mut router, recorder) = status_router();

    mode_changed(&mut router, SESSION, false);
    router.handle_foreground(vec![(9999, 8888)]);
    assert_eq!(recorder.calls().last(), Some(&None));

    let (pid, tid) = host_of(SESSION);
    // 线程号撞上、进程号不对：不算，状态条保持收起。
    router.handle_foreground(vec![(tid, 8888)]);
    assert_eq!(recorder.calls().last(), Some(&None));
    // 两个都对上才认。
    router.handle_foreground(vec![(tid, pid)]);
    assert_eq!(recorder.calls().last(), Some(&Some("拼".to_owned())));
}

/// 状态条上点出的目标模式只交给前台会话：所有装了青简的应用都在轮询，谁先来给谁就切进别的应用里去了。
#[test]
fn pending_mode_goes_only_to_the_foreground_session() {
    let mut router = router_with(RouterConfig {
        status_enabled: true,
        ..RouterConfig::default()
    });
    let notepad = SessionId(2);
    open_session(&mut router, notepad, None);
    mode_changed(&mut router, SESSION, false);
    mode_changed(&mut router, notepad, false);
    focus_window(&mut router, SESSION);

    router.handle_status_event(StatusEvent::ToggleMode);
    // 后台那个会话来问：不给。
    assert_eq!(
        router.handle(ClientMessage::SyncMode { session: notepad }),
        Some(ServerMessage::ModeSync {
            session: notepad,
            english: None,
            input: InputSettings::default(),
        })
    );
    // 前台会话来问：给它。
    assert_eq!(
        router.handle(ClientMessage::SyncMode { session: SESSION }),
        Some(ServerMessage::ModeSync {
            session: SESSION,
            english: Some(true),
            input: InputSettings::default(),
        })
    );
}

/// 换了前台，上一个应用还没取走的切换请求作废，别把它带给新前台。
#[test]
fn pending_mode_is_dropped_when_the_foreground_changes() {
    let mut router = router_with(RouterConfig {
        status_enabled: true,
        ..RouterConfig::default()
    });
    let notepad = SessionId(2);
    open_session(&mut router, notepad, None);
    mode_changed(&mut router, SESSION, false);
    mode_changed(&mut router, notepad, false);
    focus_window(&mut router, SESSION);

    router.handle_status_event(StatusEvent::ToggleMode);
    focus_window(&mut router, notepad);
    for session in [SESSION, notepad] {
        assert_eq!(
            router.handle(ClientMessage::SyncMode { session }),
            Some(ServerMessage::ModeSync {
                session,
                english: None,
                input: InputSettings::default(),
            })
        );
    }
}

/// 状态条模式格按「拼音侧一个字 + 形码的『五』」显示：双拼是 `双`、五笔开着再跟一个 `五` ——
/// **方案全名不进状态条**（`中 · 小浪双拼` 那种写法会随配置变长变短、把整条撑宽，全名去设置页「通用」看）。
#[test]
fn status_bar_mode_cell_shows_the_scheme_letter() {
    let config = RouterConfig {
        status_enabled: true,
        scheme: Scheme::Shuangpin(ShuangpinScheme::Xiaohe),
        wubi: true,
        ..RouterConfig::default()
    };
    let mut router = router_with(config);
    let recorder = RecordingStatus::default();
    router.set_status_sink(Box::new(recorder.clone()));

    mode_changed(&mut router, SESSION, false);

    assert_eq!(recorder.calls(), vec![Some("双五".to_owned())]);
}

/// Caps Lock 亮着时模式格只出「A」（不跟汉字挤一格）；灭了就回到方案那串。
#[test]
fn status_bar_shows_caps_lock_in_the_mode_cell() {
    let (mut router, recorder) = status_router();
    mode_changed_with(&mut router, SESSION, false, true);
    assert_eq!(recorder.calls().last(), Some(&Some("A".to_owned())));

    mode_changed_with(&mut router, SESSION, true, true);
    assert_eq!(recorder.calls().last(), Some(&Some("A".to_owned())));

    mode_changed_with(&mut router, SESSION, false, false);
    assert_eq!(recorder.calls().last(), Some(&Some("拼".to_owned())));
}

#[test]
fn status_bar_stays_hidden_when_disabled() {
    let mut router = router();
    let recorder = RecordingStatus::default();
    router.set_status_sink(Box::new(recorder.clone()));

    router.handle(ClientMessage::ModeChanged {
        session: SESSION,
        english: false,
        caps: false,
    });

    assert_eq!(recorder.calls(), vec![None]);
}

#[test]
fn deleting_a_candidate_shows_a_notice_until_next_key() {
    let mut router = router();
    let (_, _, frame) = type_letters(&mut router, "nihao");
    let slot = slot_of(&frame, "你好");

    // 「你好」是词库词且没学习记录，删不掉，但提示照样给出。
    let (outcome, _, after) = press(&mut router, digit_with(slot, SHIFT));
    assert_eq!(outcome, KeyOutcome::Consumed);
    assert!(!after.is_empty(), "删候选后仍在组句");
    let notice = after.notice.as_deref().expect("删候选后应带屏幕提示");
    assert!(notice.contains("你好"), "提示应提到候选词，实际：{notice}");

    let (_, _, next) = press(&mut router, KeyEvent::new(0x28, None, Default::default())); // VK_DOWN
    assert_eq!(next.notice, None, "提示应只活到下一次按键");
}

#[test]
fn expression_mode_takes_digits_and_operators() {
    let mut router = router();
    // v 开头进表达式模式：数字不选词、运算符进算式，Shift + 6 是 `^` 而不是删候选键。
    type_letters(&mut router, "v");
    press(&mut router, digit(1));
    press(&mut router, punct('+'));
    let (outcome, commit, frame) = press(&mut router, digit(2));
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert_eq!(preedit(&frame), "v1+2");
    assert_eq!(candidate_texts(&frame), ["3", "1+2=3"]);
    press(&mut router, digit_with(6, SHIFT));
    let (_, _, frame) = press(&mut router, digit(2));
    assert_eq!(preedit(&frame), "v1+2^2");
    assert_eq!(candidate_texts(&frame), ["5", "1+2^2=5"]);
    // 空格上屏首选并清空。
    let (outcome, commit, frame) = press(&mut router, punct(' '));
    assert_eq!(
        (outcome, commit),
        (KeyOutcome::Consumed, Some("5".to_owned()))
    );
    assert!(preedit(&frame).is_empty());
}

#[test]
fn expression_mode_spells_chinese_numerals() {
    let mut router = router();
    type_letters(&mut router, "v");
    for n in [1, 2, 3] {
        press(&mut router, digit(n));
    }
    let (_, _, frame) = press(&mut router, punct('.'));
    assert_eq!(preedit(&frame), "v123.");
    let (_, _, frame) = press(&mut router, digit(5));
    assert_eq!(
        candidate_texts(&frame),
        [
            "一百二十三点五",
            "壹佰贰拾叁点伍",
            "一百二十三元五角",
            "壹佰贰拾叁元伍角"
        ]
    );
    router.handle(ClientMessage::Key {
        session: SESSION,
        event: KeyEvent::new(0x1B, None, Default::default()),
    });
    type_letters(&mut router, "v");
    for n in [1, 2, 3] {
        press(&mut router, digit(n));
    }
    let (_, _, frame) = press(&mut router, punct('+'));
    // `v123+` 算不出来就没有候选，回车上屏原文。
    assert!(candidate_texts(&frame).is_empty());
    let (_, _, frame) = press(&mut router, KeyEvent::new(0x08, None, Default::default()));
    assert_eq!(
        candidate_texts(&frame),
        [
            "一百二十三",
            "壹佰贰拾叁",
            "一百二十三元整",
            "壹佰贰拾叁元整"
        ]
    );
    press(&mut router, KeyEvent::new(0x28, None, Default::default()));
    let (_, commit, _) = press(&mut router, punct(' '));
    assert_eq!(commit.as_deref(), Some("壹佰贰拾叁"));
}

#[test]
fn expression_mode_other_punctuation_commits_then_applies() {
    let mut router = router();
    type_letters(&mut router, "v");
    press(&mut router, digit(1));
    press(&mut router, punct('+'));
    press(&mut router, digit(2));
    // 逗号不是算式的一部分：先把首选上屏，逗号按没在组句处理（中文模式转全角）。
    let (outcome, commit, frame) = press(&mut router, punct(','));
    assert_eq!(
        (outcome, commit),
        (KeyOutcome::Consumed, Some("3，".to_owned()))
    );
    assert!(preedit(&frame).is_empty());
}

#[test]
fn question_key_unicode_entry_takes_digits() {
    let mut router = router();
    type_letters(&mut router, "u");
    press(&mut router, digit(4));
    type_letters(&mut router, "e");
    press(
        &mut router,
        KeyEvent::new(0x30, Some('0'), Default::default()),
    );
    let (_, _, frame) = press(
        &mut router,
        KeyEvent::new(0x30, Some('0'), Default::default()),
    );
    assert_eq!(preedit(&frame), "u4e00");
    assert_eq!(candidate_texts(&frame), ["一"]);
    let (_, commit, _) = press(&mut router, punct(' '));
    assert_eq!(commit.as_deref(), Some("一"));
    // `u+1f600`：`+` 也进缓冲区。
    type_letters(&mut router, "u");
    press(&mut router, punct('+'));
    press(&mut router, digit(1));
    type_letters(&mut router, "f");
    press(&mut router, digit(6));
    press(
        &mut router,
        KeyEvent::new(0x30, Some('0'), Default::default()),
    );
    let (_, _, frame) = press(
        &mut router,
        KeyEvent::new(0x30, Some('0'), Default::default()),
    );
    assert_eq!(candidate_texts(&frame), ["😀"]);
}

fn function_key(virtual_key: u32) -> KeyEvent {
    KeyEvent::new(virtual_key, None, Default::default())
}

#[test]
fn bare_question_mark_is_plain_punctuation_by_default() {
    let mut router = router();
    // 缺省 `?` 不进问字：中文模式直接出全角问号，英文模式半角。
    let (outcome, commit, frame) = press(&mut router, punct('?'));
    assert_eq!(outcome, KeyOutcome::Consumed);
    assert_eq!(commit.as_deref(), Some("？"));
    assert!(preedit(&frame).is_empty());
    let (outcome, commit, _) = press(&mut router, KeyEvent::new(0xBF, Some('?'), ENGLISH));
    assert_eq!((outcome, commit), (KeyOutcome::Passthrough, None));
}

#[test]
fn bare_question_mark_enters_question_mode_in_both_modes() {
    let mut router = router_asking();
    // 中文模式：`?` 进问字模式不上屏，后面的字母是问题。
    let (outcome, commit, frame) = press(&mut router, punct('?'));
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert_eq!(preedit(&frame), "?");
    let (_, commit, frame) = type_letters(&mut router, "sangemu");
    assert_eq!(commit, None);
    assert!(preedit(&frame).starts_with('?'), "{}", preedit(&frame));
    press(&mut router, function_key(0x1B));
    // 英文模式也一样，Caps 送来的大写字母按小写收。
    let (outcome, commit, frame) = press(&mut router, KeyEvent::new(0xBF, Some('?'), CAPS));
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert_eq!(preedit(&frame), "?");
    let (_, _, frame) = press(&mut router, letter_with('S', CAPS));
    assert_eq!(preedit(&frame), "?s");
}

#[test]
fn bare_question_mark_restores_when_followed_by_other_keys() {
    let mut router = router_asking();
    // 空格只是把这个 ? 上屏（中文模式全角），不多打空格。
    press(&mut router, punct('?'));
    let (outcome, commit, frame) = press(&mut router, punct(' '));
    assert_eq!(
        (outcome, commit),
        (KeyOutcome::Consumed, Some("？".to_owned()))
    );
    assert!(preedit(&frame).is_empty());
    // 回车同样只上屏问号，吞掉回车。
    press(&mut router, punct('?'));
    let (outcome, commit, _) = press(&mut router, function_key(0x0D));
    assert_eq!(
        (outcome, commit),
        (KeyOutcome::Consumed, Some("？".to_owned()))
    );
    // 其他字符：问号上屏后按没在组句处理（逗号转全角）。
    press(&mut router, punct('?'));
    let (_, commit, _) = press(&mut router, punct(','));
    assert_eq!(commit.as_deref(), Some("？，"));
    // 退格删掉它，什么都不上屏。
    press(&mut router, punct('?'));
    let (outcome, commit, frame) = press(&mut router, function_key(0x08));
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert!(preedit(&frame).is_empty());
    // 英文模式还原成半角。
    press(&mut router, KeyEvent::new(0xBF, Some('?'), ENGLISH));
    let (_, commit, _) = press(&mut router, KeyEvent::new(0x20, Some(' '), ENGLISH));
    assert_eq!(commit.as_deref(), Some("?"));
}

#[test]
fn bare_question_mark_is_half_width_when_full_width_is_off() {
    let mut router = router_asking_with(RouterConfig {
        full_width: false,
        ..RouterConfig::default()
    });
    press(&mut router, punct('?'));
    let (_, commit, _) = press(&mut router, punct(' '));
    assert_eq!(commit.as_deref(), Some("?"));
}

#[test]
fn shuangpin_semicolon_stays_in_buffer_in_question_mode() {
    let mut router = router_asking_with(RouterConfig {
        scheme: Scheme::Shuangpin(ShuangpinScheme::Microsoft),
        ..RouterConfig::default()
    });
    // 微软双拼的 `;` 是 ing 键：问字模式下末尾有落单声母时进缓冲区，而不是把候选上屏。
    press(&mut router, punct('?'));
    type_letters(&mut router, "x");
    let (outcome, commit, frame) = press(&mut router, punct(';'));
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    assert!(!preedit(&frame).is_empty());
    assert_ne!(preedit(&frame), "?x");
}

#[test]
fn shuangpin_enters_modes_with_shifted_letters() {
    let mut router = router_with(RouterConfig {
        scheme: Scheme::Shuangpin(ShuangpinScheme::Xiaohe),
        ..RouterConfig::default()
    });
    // Shift+V 进表达式：数字和运算符进缓冲区，空格上屏结果。
    let (outcome, commit, _) = press(&mut router, letter_with('V', SHIFT));
    assert_eq!((outcome, commit), (KeyOutcome::Consumed, None));
    press(&mut router, digit(1));
    press(&mut router, punct('+'));
    let (_, _, frame) = press(&mut router, digit(2));
    assert_eq!(preedit(&frame), "V1+2");
    assert_eq!(candidate_texts(&frame)[0], "3");
    let (_, commit, _) = press(&mut router, punct(' '));
    assert_eq!(commit.as_deref(), Some("3"));
    // Shift+U 进问字：码点本地答。
    press(&mut router, letter_with('U', SHIFT));
    press(&mut router, digit(4));
    type_letters(&mut router, "e");
    press(&mut router, digit(0));
    let (_, _, frame) = press(&mut router, digit(0));
    assert_eq!(candidate_texts(&frame), ["一"]);
    press(&mut router, function_key(0x1B));
    // 小写 v 仍是音节键；其他大写字母、全拼下的 Shift+V 照旧交给应用。
    let (_, _, frame) = type_letters(&mut router, "v");
    assert_eq!(preedit(&frame), "zh");
    press(&mut router, function_key(0x1B));
    assert_eq!(
        press(&mut router, letter_with('A', SHIFT)).0,
        KeyOutcome::Passthrough
    );
    let mut full = router_with(RouterConfig::default());
    let (outcome, _, frame) = press(&mut full, letter_with('V', SHIFT));
    assert_eq!(outcome, KeyOutcome::Passthrough);
    assert!(frame.is_empty());
}

#[test]
fn punctuation_toggle_is_remembered_per_mode() {
    let mut router = router_with(RouterConfig {
        status_enabled: true,
        ..RouterConfig::default()
    });
    let comma = KeyEvent::new(0xBC, Some(','), Default::default());
    let english_comma = KeyEvent::new(0xBC, Some(','), ENGLISH);
    // 中文模式下切成半角。
    router.handle(ClientMessage::ModeChanged {
        session: SESSION,
        english: false,
        caps: false,
    });
    router.handle_status_event(StatusEvent::TogglePunctuation);
    assert_eq!(press(&mut router, comma).0, KeyOutcome::Passthrough);
    // 英文模式缺省半角；点那一格切成全角，英文模式下真转。
    router.handle(ClientMessage::ModeChanged {
        session: SESSION,
        english: true,
        caps: false,
    });
    assert_eq!(press(&mut router, english_comma).0, KeyOutcome::Passthrough);
    router.handle_status_event(StatusEvent::TogglePunctuation);
    assert_eq!(press(&mut router, english_comma).1, Some("，".to_owned()));
    // 切回中文：还是中文自己记住的半角；再切回英文：还是英文记住的全角。
    router.handle(ClientMessage::ModeChanged {
        session: SESSION,
        english: false,
        caps: false,
    });
    assert_eq!(press(&mut router, comma).0, KeyOutcome::Passthrough);
    router.handle(ClientMessage::ModeChanged {
        session: SESSION,
        english: true,
        caps: false,
    });
    assert_eq!(press(&mut router, english_comma).1, Some("，".to_owned()));
    // 英文候选组词中敲标点：先把字母原样上屏，标点也按英文那份转。
    type_english(&mut router, "hello");
    let (_, commit, _) = press(&mut router, english_comma);
    assert_eq!(commit.as_deref(), Some("hello，"));
}

/// 假打分器：偏爱某个文本，其余都给低分（与 Core 的重打分测试同款）。
struct Prefers(&'static str);

impl SentenceScorer for Prefers {
    fn score(&self, _context: &str, texts: &[&str]) -> Vec<f64> {
        texts
            .iter()
            .map(|t| if *t == self.0 { -1.0 } else { -20.0 })
            .collect()
    }
}

/// 接了假模型的 Router：本地整句模型在壳里是异步接法，按键先按词级出候选，停顿后 tick 才换。
fn router_with_scorer(preferred: &'static str) -> Router {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut engine = assembly::assemble(&AssemblySpec::new(root.join("assets/sample/dict.tsv")))
        .expect("assemble engine from sample data");
    engine.set_async_sentence_scorer(Some(Box::new(Prefers(preferred))));
    let mut router = Router::new(engine, RouterConfig::default());
    open_session(&mut router, SESSION, None);
    router
}

/// 一直 tick 到首选变成 `text` 或等满 `timeout`；返回最后一帧。
fn tick_until_first(router: &mut Router, text: &str, timeout: std::time::Duration) -> Frame {
    let started = std::time::Instant::now();
    loop {
        std::thread::sleep(router.next_tick().min(std::time::Duration::from_millis(20)));
        router.tick();
        let frame = match router.handle(ClientMessage::Poll { session: SESSION }) {
            Some(ServerMessage::Update { frame, .. }) => frame,
            other => panic!("expected Update, got {other:?}"),
        };
        if candidate_texts(&frame).first() == Some(&text) || started.elapsed() > timeout {
            return frame;
        }
    }
}

#[test]
fn local_model_rescoring_reorders_sentence_after_pause() {
    // k 优路径按末词分状态，几条路径要在末词上不同才都留下来：ni + ta → 你他 / 你她 / 你它
    let mut router = router_with_scorer("你它");
    let (_, _, frame) = type_letters(&mut router, "nita");
    // 按键时只按词级模型：他 的词频高，首选是「你他」
    assert_eq!(candidate_texts(&frame).first(), Some(&"你他"));
    // 在等防抖，工人循环该在 80 ms 内醒来
    assert!(router.next_tick() <= std::time::Duration::from_millis(80));

    let frame = tick_until_first(&mut router, "你它", std::time::Duration::from_secs(3));
    assert_eq!(
        candidate_texts(&frame).first(),
        Some(&"你它"),
        "停顿后模型偏爱的整句应换到首位，实际：{:?}",
        candidate_texts(&frame)
    );
    // 换完不再等；空闲节拍回到看配置文件的一秒
    assert_eq!(router.next_tick(), std::time::Duration::from_secs(1));
}

#[test]
fn local_model_does_not_touch_a_navigated_page() {
    let mut router = router_with_scorer("你它");
    type_letters(&mut router, "nita");
    // 用户动过高亮：模型的结果只留在缓存里，不换正在看的这页
    let (_, _, frame) = press(&mut router, KeyEvent::new(0x28, None, Default::default())); // VK_DOWN
    assert_eq!(candidate_texts(&frame).first(), Some(&"你他"));
    // 防抖 80 ms + 假模型立即回分，300 ms 足够等到结果；首选仍是原来的
    let frame = tick_until_first(&mut router, "你它", std::time::Duration::from_millis(300));
    assert_eq!(candidate_texts(&frame).first(), Some(&"你他"));
}

#[test]
fn surrounding_text_arriving_after_the_first_key_still_rescoring() {
    let mut router = router_with_scorer("你它");
    type_letters(&mut router, "nita");
    // DLL 在起组句的编辑会话里读到前文、按键之后才送来：前文换了，缓存按旧前文记的作废，要能重新排期
    assert_eq!(
        router.handle(ClientMessage::Surrounding {
            session: SESSION,
            text: "今天".to_owned(),
            after: String::new(),
        }),
        None
    );
    assert!(router.next_tick() <= std::time::Duration::from_millis(80));
    let frame = tick_until_first(&mut router, "你它", std::time::Duration::from_secs(3));
    assert_eq!(candidate_texts(&frame).first(), Some(&"你它"));
    // 别的会话送来的前文不影响聚焦会话
    assert_eq!(
        router.handle(ClientMessage::Surrounding {
            session: SessionId(9),
            text: "无关".to_owned(),
            after: String::new(),
        }),
        None
    );
}

/// DLL 报来「私密输入框」：Engine 进私密（不学不记不发云端），焦点换到别的会话按那个会话的状态重设，切回来再进。
#[test]
fn privacy_follows_the_focused_session() {
    let mut router = router();
    // 真实顺序：第一键起组句，DLL 在那次编辑会话里判出私密再报来
    let (outcome, commit, _) = type_letters(&mut router, "kaifa");
    assert_eq!(outcome, KeyOutcome::Consumed);
    assert_eq!(commit, None);
    assert!(!router.is_private());
    assert_eq!(
        router.handle(ClientMessage::Privacy {
            session: SESSION,
            private: true,
        }),
        None
    );
    assert!(router.is_private());
    // 私密中照常上屏
    let (_, commit, _) = press(&mut router, digit(1));
    assert!(commit.is_some());
    // 另一个会话开进来拿焦点：它不私密
    open_session(&mut router, SessionId(2), None);
    press_in(&mut router, SessionId(2), letter('k'));
    assert!(!router.is_private());
    // 焦点回到第一个会话：仍是私密
    press_in(&mut router, SESSION, letter('k'));
    assert!(router.is_private());
    // 报不私密了
    assert_eq!(
        router.handle(ClientMessage::Privacy {
            session: SESSION,
            private: false,
        }),
        None
    );
    assert!(!router.is_private());
    // 别的会话的私密状态不影响聚焦会话
    assert_eq!(
        router.handle(ClientMessage::Privacy {
            session: SessionId(9),
            private: true,
        }),
        None
    );
    assert!(!router.is_private());
}

fn press_in(router: &mut Router, session: SessionId, event: KeyEvent) {
    let _ = router.handle(ClientMessage::Key { session, event });
}

/// 候选窗点选候选：Router 立刻把词选掉，但要上屏的文本得等 DLL 下一次轮询取走
///（传输一问一答，Server 不能主动推）。
#[test]
fn candidate_click_hands_the_text_to_the_next_poll() {
    let mut router = router();
    let (_, _, frame) = type_letters(&mut router, "nihao");
    let first = frame.candidates.items.first().unwrap().text.clone();

    router.handle_candidate_event(CandidateEvent::Pick(0));

    match router.handle(ClientMessage::Poll { session: SESSION }) {
        Some(ServerMessage::Update { commit, frame, .. }) => {
            assert_eq!(
                commit.as_deref(),
                Some(first.as_str()),
                "点选的词由轮询带回"
            );
            assert!(frame.is_empty(), "整段拼音吃完，该收起候选窗");
        }
        other => panic!("expected update, got {other:?}"),
    }
    // 取走即清，别把同一个词上屏两次。
    match router.handle(ClientMessage::Poll { session: SESSION }) {
        Some(ServerMessage::Update { commit, .. }) => assert_eq!(commit, None),
        other => panic!("expected update, got {other:?}"),
    }
}

/// 假云联想：请求照收，结果要测试这边点头（`deliver`）才到——真实那条链路是网络的异步延迟，
/// 这样才看得见「☁ …」那一拍。
#[derive(Default)]
struct FakeCloud {
    /// 每次请求的 `want_sentence`，按提交顺序。
    asked: Vec<bool>,
    /// 每次请求带的光标前文，按提交顺序。
    befores: Vec<String>,
    /// 每次请求带的光标后文，按提交顺序。
    afters: Vec<String>,
    /// 每次请求带的拼音，按提交顺序；续写那条路是空串。
    pinies: Vec<String>,
    /// 最近一次请求的序号：`deliver` 用它造结果。
    last_request: Option<u64>,
    /// 备好、等轮询取走的结果。
    reply: Option<Prediction>,
}

struct FakePredictor {
    /// 自动那一路（`sentence_trigger = "idle"`）要不要句子，对应 `[predict] sentence`。
    auto_sentence: bool,
    cloud: Arc<Mutex<FakeCloud>>,
}

impl Predictor for FakePredictor {
    fn policy(&self) -> PredictionPolicy {
        PredictionPolicy {
            sentence: self.auto_sentence,
            ..PredictionPolicy::default()
        }
    }

    fn submit(&mut self, request: PredictionRequest) {
        let mut cloud = self.cloud.lock().unwrap();
        cloud.asked.push(request.want_sentence);
        cloud.befores.push(request.before);
        cloud.afters.push(request.after);
        cloud.pinies.push(request.pinyin);
        cloud.last_request = Some(request.sequence);
    }

    fn poll(&mut self) -> Option<Prediction> {
        self.cloud.lock().unwrap().reply.take()
    }
}

const CLOUD_SENTENCE: &str = "你好，很高兴认识你！";

/// 备一条整句，等下一次轮询取走；得先有请求才知道序号。
fn deliver(cloud: &Arc<Mutex<FakeCloud>>, sentence: &str) {
    let mut cloud = cloud.lock().unwrap();
    let sequence = cloud.last_request.expect("还没有请求，哪来的结果");
    cloud.reply = Some(Prediction {
        sequence,
        words: Vec::new(),
        sentence: Some(sentence.to_owned()),
    });
}

/// 装了假云联想的 Router（`sentence_on_tab` 决定自动那一路要不要句子）。
fn router_with_cloud(
    sentence_on_tab: bool,
    auto_sentence: bool,
) -> (Router, Arc<Mutex<FakeCloud>>) {
    let config = RouterConfig {
        sentence_on_tab,
        ..RouterConfig::default()
    };
    let cloud = Arc::new(Mutex::new(FakeCloud::default()));
    let mut router = router_in(config, None);
    router.engine_mut().set_predictor(Box::new(FakePredictor {
        auto_sentence,
        cloud: cloud.clone(),
    }));
    (router, cloud)
}

fn tab() -> KeyEvent {
    KeyEvent::new(0x09, None, Default::default())
}

fn poll(router: &mut Router) -> Frame {
    match router.handle(ClientMessage::Poll { session: SESSION }) {
        Some(ServerMessage::Update { frame, .. }) => frame,
        other => panic!("expected update, got {other:?}"),
    }
}

/// 「按 Tab 才联想」：敲拼音那一拍不问句子，按 Tab 才现请，请出去先摆「☁ …」等结果。
#[test]
fn tab_asks_for_the_sentence_and_shows_the_waiting_hint() {
    let (mut router, cloud) = router_with_cloud(true, false);
    type_letters(&mut router, "nihao");

    let (outcome, commit, frame) = press(&mut router, tab());
    assert_eq!(outcome, KeyOutcome::Consumed, "Tab 被吃掉，别去缩进");
    assert_eq!(commit, None, "结果还没到，这一拍不上屏");
    assert!(frame.sentence.is_none());
    assert!(frame.sentence_pending, "候选窗摆出「☁ …」");
    let asked = cloud.lock().unwrap().asked.clone();
    assert_eq!(asked.last(), Some(&true), "Tab 这一拍才破例要句子");
    assert!(
        asked[..asked.len() - 1].iter().all(|want| !want),
        "自动那一路按配置不带句子：{asked:?}"
    );

    deliver(&cloud, CLOUD_SENTENCE);
    let frame = poll(&mut router);
    assert_eq!(frame.sentence.as_deref(), Some(CLOUD_SENTENCE));
    assert!(!frame.sentence_pending, "结果到了，提示收掉");
}

/// 结果回来后再按一次 Tab：整句上屏，拼音清空。
#[test]
fn second_tab_commits_the_sentence() {
    let (mut router, cloud) = router_with_cloud(true, false);
    type_letters(&mut router, "nihao");
    press(&mut router, tab());
    deliver(&cloud, CLOUD_SENTENCE);
    poll(&mut router);

    let (outcome, commit, frame) = press(&mut router, tab());
    assert_eq!(outcome, KeyOutcome::Consumed);
    assert_eq!(commit.as_deref(), Some(CLOUD_SENTENCE));
    assert!(frame.is_empty(), "整段拼音吃完，候选窗收起");
}

/// 续写：敲续写键（缺省 `i`）再按 Tab，前缀不进拼音、请求只带光标前后文；
/// 结果在 raw 状态（没有拼音候选）下也要显示，再按一次 Tab 采用。
#[test]
fn continue_key_then_tab_asks_for_a_continuation_without_pinyin() {
    let (mut router, cloud) = router_with_cloud(true, false);
    router.handle(ClientMessage::Surrounding {
        session: SESSION,
        text: "笛卡儿积是一种二元运算，把两个集合".to_owned(),
        after: "按顺序两两配对组成有序对。".to_owned(),
    });
    type_letters(&mut router, "i");

    let (outcome, commit, frame) = press(&mut router, tab());
    assert_eq!(outcome, KeyOutcome::Consumed, "Tab 被吃掉");
    assert_eq!(commit, None, "结果还没到，这一拍不上屏");
    assert!(frame.sentence_pending, "候选窗摆出「☁ …」");
    {
        let cloud = cloud.lock().unwrap();
        assert_eq!(cloud.asked.last(), Some(&true), "Tab 这一拍才要句子");
        assert_eq!(
            cloud.befores.last().map(String::as_str),
            Some("笛卡儿积是一种二元运算，把两个集合"),
            "续写要带光标前文"
        );
        assert_eq!(
            cloud.afters.last().map(String::as_str),
            Some("按顺序两两配对组成有序对。"),
            "光标后文也要带上"
        );
    }

    deliver(&cloud, "中的元素");
    let frame = poll(&mut router);
    assert_eq!(
        frame.sentence.as_deref(),
        Some("中的元素"),
        "raw 状态（敲了续写键）下续写结果也要显示"
    );

    let (_, commit, _) = press(&mut router, tab());
    assert_eq!(commit.as_deref(), Some("中的元素"), "再按一次 Tab 采用");
}

/// 光标前后文**按会话各记一份**：别的会话（后台应用）后来报的那份不能顶掉当前会话的。
/// 2026-09-18 的诊断日志实证过这个串味：A 应用里复现续写期间，B 应用每敲一个字都在覆盖它。
#[test]
fn surrounding_is_kept_per_session() {
    const OTHER: SessionId = SessionId(2);
    let (mut router, cloud) = router_with_cloud(true, false);
    open_session(&mut router, OTHER, None);
    router.handle(ClientMessage::Surrounding {
        session: SESSION,
        text: "把两个集合".to_owned(),
        after: "按顺序两两配对。".to_owned(),
    });
    // 另一个会话后报、内容完全不同：旧实现（Router 上全局一份）会被它顶掉。
    router.handle(ClientMessage::Surrounding {
        session: OTHER,
        text: "另一个窗口正在打的字".to_owned(),
        after: String::new(),
    });

    type_letters(&mut router, "i");
    let (outcome, _, frame) = press(&mut router, tab());
    assert_eq!(outcome, KeyOutcome::Consumed, "Tab 被吃掉");
    assert!(frame.sentence_pending, "候选窗摆出「☁ …」");
    let cloud = cloud.lock().unwrap();
    assert_eq!(
        cloud.befores.last().map(String::as_str),
        Some("把两个集合"),
        "用的必须是当前会话那份前后文"
    );
    assert_eq!(
        cloud.afters.last().map(String::as_str),
        Some("按顺序两两配对。")
    );
}

/// 读到空要**作废**上一次那份：不能拿旧文本接着续写（DLL 现在空报也会送一条过来）。
#[test]
fn empty_surrounding_clears_the_previous_one() {
    let (mut router, cloud) = router_with_cloud(true, false);
    router.handle(ClientMessage::Surrounding {
        session: SESSION,
        text: "特朗普在社交媒体上发文表示".to_owned(),
        after: String::new(),
    });
    // 光标移到应用给不出上下文的位置：空报（以前这条会被丢掉，旧文本继续沿用）。
    router.handle(ClientMessage::Surrounding {
        session: SESSION,
        text: String::new(),
        after: String::new(),
    });

    type_letters(&mut router, "i");
    let (_, _, frame) = press(&mut router, tab());
    assert!(!frame.sentence_pending, "没有上下文就不该去请续写");
    assert!(
        cloud.lock().unwrap().befores.iter().all(String::is_empty),
        "不能带上一次那份旧前后文"
    );
}

/// 双拼下续写键换成 `Shift + I`：小写 `i` 是音节键，入口得按住 Shift 敲大写。
/// `ModeKeys::shifted` 与 DLL 早就算上了这个键，Core 的入口判定漏了它，双拼下续写整个进不去。
#[test]
fn shuangpin_continue_key_is_the_shifted_letter() {
    let cloud = Arc::new(Mutex::new(FakeCloud::default()));
    let config = RouterConfig {
        sentence_on_tab: true,
        scheme: Scheme::Shuangpin(ShuangpinScheme::Xiaohe),
        ..RouterConfig::default()
    };
    let mut router = router_in(config, None);
    router.engine_mut().set_predictor(Box::new(FakePredictor {
        auto_sentence: false,
        cloud: cloud.clone(),
    }));
    router.handle(ClientMessage::Surrounding {
        session: SESSION,
        text: "把两个集合".to_owned(),
        after: "按顺序两两配对。".to_owned(),
    });

    // 小写 `i` 在双拼里是音节键：这一拍走的是拼音那条路（请求带解出来的拼音），不是续写
    type_letters(&mut router, "i");
    let (_, _, frame) = press(&mut router, tab());
    assert!(frame.sentence_pending, "Tab 现请整句");
    assert!(
        cloud
            .lock()
            .unwrap()
            .pinies
            .last()
            .is_some_and(|pinyin| !pinyin.is_empty()),
        "小写 i 不是续写，请求该带拼音"
    );
    press(&mut router, function_key(0x1B));

    // 按住 Shift 的大写 `I` 才是入口
    let (outcome, _, _) = press(&mut router, letter_with('I', SHIFT));
    assert_eq!(outcome, KeyOutcome::Consumed, "Shift + I 被吃掉");
    let (outcome, commit, frame) = press(&mut router, tab());
    assert_eq!(outcome, KeyOutcome::Consumed, "Tab 被吃掉");
    assert_eq!(commit, None, "结果还没到，这一拍不上屏");
    assert!(frame.sentence_pending, "候选窗摆出「☁ …」");
    {
        let cloud = cloud.lock().unwrap();
        assert_eq!(
            cloud.befores.last().map(String::as_str),
            Some("把两个集合"),
            "续写要带光标前文"
        );
        assert_eq!(
            cloud.pinies.last().map(String::as_str),
            Some(""),
            "续写不带拼音"
        );
    }
}

/// 没配「按 Tab 才联想」：句子照样能用 Tab 接受（自动那一路请回来的）。
#[test]
fn tab_accepts_a_sentence_that_arrived_automatically() {
    let (mut router, cloud) = router_with_cloud(false, true);
    type_letters(&mut router, "nihao");
    assert_eq!(
        cloud.lock().unwrap().asked.last(),
        Some(&true),
        "自动那一路本来就带句子"
    );

    deliver(&cloud, CLOUD_SENTENCE);
    poll(&mut router);

    let (_, commit, _) = press(&mut router, tab());
    assert_eq!(commit.as_deref(), Some(CLOUD_SENTENCE));
}

/// 云联想要带上下文：DLL 在组句起始时送来的光标前后文，每一次请求都要跟着走。
/// 没有它模型只能按拼音硬猜，给出来的句子常常接不上用户正在写的话题。
#[test]
fn cloud_requests_carry_the_surrounding_text() {
    let (mut router, cloud) = router_with_cloud(true, false);
    router.handle(ClientMessage::Surrounding {
        session: SESSION,
        text: "我们今天".to_owned(),
        after: "开会".to_owned(),
    });
    type_letters(&mut router, "nihao");
    let cloud = cloud.lock().unwrap();
    assert!(!cloud.befores.is_empty(), "敲了拼音就该有请求");
    assert!(
        cloud.befores.iter().all(|b| b == "我们今天"),
        "每次都该带上光标前文，实际：{:?}",
        cloud.befores
    );
    assert!(
        cloud.afters.iter().all(|a| a == "开会"),
        "光标后文也要带上，实际：{:?}",
        cloud.afters
    );
}

/// 云联想关着：没有整句补全时 Tab 翻下一页（#160），不再交还应用。
#[test]
fn tab_pages_when_cloud_is_off() {
    let config = RouterConfig {
        sentence_on_tab: true,
        ..RouterConfig::default()
    };
    let mut router = router_in(config, None);
    type_letters(&mut router, "nihao");

    let (outcome, commit, frame) = press(&mut router, tab());
    assert_eq!(outcome, KeyOutcome::Consumed, "无补全时 Tab 翻页并吞掉");
    assert_eq!(commit, None);
    assert!(!frame.sentence_pending, "没接联想，不该摆等待提示");
}

/// 中文模式下的 Shift 大写：缺省交给应用（与以前一致），配成 compose 才收进组句缓冲区。
#[test]
fn shift_letters_follow_the_configuration() {
    // 缺省 `shift_letter = "passthrough"`：临时打英文，字母归应用
    let mut router = router();
    let (outcome, commit, _) = press(&mut router, letter_with('P', SHIFT));
    assert_eq!(outcome, KeyOutcome::Passthrough);
    assert_eq!(commit, None);

    // 配成 compose：进组句，按小写参与匹配
    let config = RouterConfig {
        shift_letter_compose: true,
        ..RouterConfig::default()
    };
    let mut router = router_with(config);
    type_letters(&mut router, "c");
    let (outcome, commit, frame) = press(&mut router, letter_with('P', SHIFT));
    assert_eq!(outcome, KeyOutcome::Consumed);
    assert_eq!(commit, None);
    assert_eq!(preedit(&frame), "cP", "大写收进组句，拼音行按敲的样子显示");
}

/// `shift_letter = "compose"` 下 Shift+U/I/V 是大写字母，不得进全拼的 u/i/v 模式。
#[test]
fn shift_uiv_compose_are_letters_not_mode_keys() {
    let config = RouterConfig {
        shift_letter_compose: true,
        ..RouterConfig::default()
    };
    let mut router = router_with(config);

    // Shift+U：不进问字，字母进组句
    let (outcome, commit, frame) = press(&mut router, letter_with('U', SHIFT));
    assert_eq!(outcome, KeyOutcome::Consumed);
    assert_eq!(commit, None);
    assert_eq!(preedit(&frame), "U", "Shift+U 进组句，不进问字模式");
    assert!(!router.engine_mut().question_mode());
    press(&mut router, function_key(0x1B));

    // 小写 u 仍是问字入口
    type_letters(&mut router, "u");
    assert!(router.engine_mut().question_mode(), "小写 u 仍是问字");
    press(&mut router, function_key(0x1B));

    // Shift+I：不进续写
    let (_, _, frame) = press(&mut router, letter_with('I', SHIFT));
    assert_eq!(preedit(&frame), "I");
    press(&mut router, function_key(0x1B));

    // Shift+V：不进表达式
    let (_, _, frame) = press(&mut router, letter_with('V', SHIFT));
    assert_eq!(preedit(&frame), "V");
    assert!(!router.engine_mut().expression_mode());
    press(&mut router, function_key(0x1B));
}

/// 中文模式没在组句时按 `-`：不放行、由壳插入——放行的键在部分宿主里到不了应用。
#[test]
fn minus_is_inserted_instead_of_passed_through() {
    let mut router = router();
    let (outcome, commit, _) = press(&mut router, punct('-'));
    assert_eq!(outcome, KeyOutcome::Consumed);
    assert_eq!(commit.as_deref(), Some("-"));
}

/// 放行的按键不能把攒着的点选文本吞掉（DLL 不碰文档），要留给下一次轮询。
#[test]
fn a_passthrough_key_keeps_the_picked_text_for_the_next_poll() {
    let mut router = router();
    type_letters(&mut router, "ni");
    router.handle_candidate_event(CandidateEvent::Pick(0));

    // 带 Win 的组合键归应用：这一下是 Passthrough。
    let win_l = KeyEvent::new(
        0x4C,
        Some('l'),
        KeyModifiers {
            win: true,
            ..Default::default()
        },
    );
    let (outcome, commit, _) = press(&mut router, win_l);
    assert_eq!(outcome, KeyOutcome::Passthrough);
    assert_eq!(commit, None, "放行的键自己不带上屏文本");

    match router.handle(ClientMessage::Poll { session: SESSION }) {
        Some(ServerMessage::Update { commit, .. }) => {
            assert!(commit.is_some(), "攒着的点选文本还在，等这一拍带回")
        }
        other => panic!("expected update, got {other:?}"),
    }
}

// ---- 注音模式（大千键位）与繁体输出 ----

/// 大千键位的注音 Router：`1`=ㄅ、`j`=ㄨ、`4`=ˋ、`5`=ㄓ。
/// 与 `main.rs` 一样，注音开关既给 Router 也给 Engine（分派看的是 Engine 的状态）。
fn router_zhuyin() -> Router {
    let mut router = router_with(RouterConfig {
        scheme: Scheme::Zhuyin,
        ..RouterConfig::default()
    });
    router.engine_mut().set_zhuyin_mode(true);
    router
}

/// 注音模式下字母与声调数字都进缓冲区：`1`(ㄅ) `j`(ㄨ) `4`(ˋ) 组出 ㄅㄨˋ，候选出「不」。
#[test]
fn zhuyin_mode_composes_letters_and_tone_digits() {
    let mut router = router_zhuyin();
    let (outcome, commit, frame) = type_letters(&mut router, "1j4");

    assert_eq!(outcome, KeyOutcome::Consumed);
    assert_eq!(commit, None, "组句期间不上屏");
    let texts = candidate_texts(&frame);
    assert!(texts.contains(&"不"), "ㄅㄨˋ 应出「不」，实际：{texts:?}");
}

/// 注音模式下数字是声调键，不拿来选词：ㄅㄨ 出候选后敲 `4`（ˋ）继续组字成 ㄅㄨˋ。
#[test]
fn zhuyin_digit_composes_a_tone_instead_of_picking() {
    let mut router = router_zhuyin();
    let (_, _, before) = type_letters(&mut router, "1j");
    assert!(
        candidate_texts(&before).contains(&"不"),
        "ㄅㄨ 应已出「不」，实际：{:?}",
        candidate_texts(&before)
    );

    let (outcome, commit, after) = press(&mut router, letter('4'));

    assert_eq!(outcome, KeyOutcome::Consumed);
    assert_eq!(commit, None, "声调键不该选词上屏");
    assert_ne!(
        preedit(&after),
        preedit(&before),
        "`4` 应作为 ˋ 进缓冲区，缓冲区要有变化"
    );
}

/// 注音模式下回车把高亮候选上屏；Shift + 回车才是把注音键原样上屏。
#[test]
fn zhuyin_return_commits_the_highlight_and_shift_return_the_raw_keys() {
    let mut router = router_zhuyin();
    let (_, _, frame) = type_letters(&mut router, "1j4");
    let texts = candidate_texts(&frame).to_vec();
    let (_, commit, _) = press(&mut router, function_key(0x0D));
    let committed = commit.expect("回车应上屏");
    assert!(
        texts.iter().any(|t| *t == committed),
        "回车应上屏候选 {texts:?}，实际：{committed}"
    );

    let mut router = router_zhuyin();
    type_letters(&mut router, "1j4");
    let (_, commit, _) = press(&mut router, KeyEvent::new(0x0D, None, SHIFT));
    assert_eq!(
        commit.as_deref(),
        Some("ㄅㄨˋ"),
        "Shift + 回车应把注音键原样上屏"
    );
}

/// 注音模式下还没定调时空格进缓冲区（大千布局里单敲 ㄓ 要按空格才成为 zhi），不直接选词。
#[test]
fn zhuyin_space_composes_while_a_tone_is_still_due() {
    let mut router = router_zhuyin();
    let (_, _, before) = press(&mut router, letter('5'));
    let before_texts = candidate_texts(&before).to_vec();

    let (outcome, commit, after) = press(&mut router, letter(' '));

    assert_eq!(outcome, KeyOutcome::Consumed);
    assert_eq!(commit, None, "还要声调时空格不该上屏");
    // 一声的符号是空白，拼音行看不出变化，所以比候选：空格被吸收后 ㄓ 成了 zhi，候选跟着变。
    assert_ne!(
        candidate_texts(&after),
        before_texts,
        "空格应作为一声吸收，候选跟着变；实际：{:?}",
        candidate_texts(&after)
    );
}

/// 繁体模式下候选与上屏都是繁体（`kaifa` → 開發）。
#[test]
fn traditional_mode_shows_and_commits_traditional_text() {
    let mut router = router();
    router.engine_mut().set_traditional_mode(true);

    let (_, _, frame) = type_letters(&mut router, "kaifa");
    let texts = candidate_texts(&frame);
    assert!(
        texts.contains(&"開發"),
        "繁体下应出「開發」，实际：{texts:?}"
    );

    let (_, commit, _) = press(&mut router, letter(' '));
    assert_eq!(commit.as_deref(), Some("開發"), "空格应上屏繁体");
}
