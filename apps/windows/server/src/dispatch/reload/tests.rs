//! 词库文件变化在配置不变时仍应进入正在运行的引擎。

use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use qingjian_core::Engine;
use qingjian_dictionary::{Dictionary, import};
use qingjian_platform::Config;

use super::CONFIG_POLL_INTERVAL;
use crate::dispatch::{DataDirs, Router, RouterConfig};

fn poll(router: &mut Router) {
    router.reload.as_mut().unwrap().last_check = Instant::now() - CONFIG_POLL_INTERVAL;
    router.poll_config_reload();
}

fn modified_at(path: &Path, seconds: u64) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
        .unwrap();
}

#[test]
fn import_replace_and_remove_without_config_changes() {
    let dir = std::env::temp_dir().join(format!("qingjian-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");
    std::fs::write(&config_path, "").unwrap();
    let config = Config::load(&config_path).unwrap();
    let original_config = std::fs::read(&config_path).unwrap();
    let mut router = Router::new(Engine::new(Dictionary::default()), RouterConfig::default());
    router.watch_config(
        &config,
        config_path.clone(),
        dir.clone(),
        DataDirs {
            user_root: Some(dir.clone()),
            user_dicts: Some(dir.join("dicts")),
            ..DataDirs::default()
        },
    );

    let source = dir.join("law.dict.yaml");
    std::fs::write(&source, "---\nname: law\n...\n合同法\the tong fa\t120\n").unwrap();
    let imported = import::import(&source, &dir.join("dicts")).unwrap();
    modified_at(&imported.path, 100);
    poll(&mut router);
    let dictionaries = router.engine.extra_dictionaries();
    assert_eq!(dictionaries.len(), 1);
    assert_eq!(
        dictionaries[0].lookup(&["he", "tong", "fa"], false)[0].text,
        "合同法"
    );

    // 只改词频，输出长度保持不变，必须靠 mtime 发现更新。
    let original_len = std::fs::metadata(&imported.path).unwrap().len();
    std::fs::write(&source, "---\nname: law\n...\n合同法\the tong fa\t121\n").unwrap();
    import::import(&source, &dir.join("dicts")).unwrap();
    modified_at(&imported.path, 200);
    assert_eq!(
        std::fs::metadata(&imported.path).unwrap().len(),
        original_len
    );
    poll(&mut router);
    assert_eq!(
        router.engine.extra_dictionaries()[0].lookup(&["he", "tong", "fa"], false)[0].frequency,
        121
    );

    // 保持旧词库映射打开，模拟运行中的 Server 收到同名替换。
    std::fs::write(
        &source,
        "---\nname: law\n...\n民法典\tmin fa dian\t100\n法律\tfa lv\t30\n",
    )
    .unwrap();
    import::import(&source, &dir.join("dicts")).unwrap();
    poll(&mut router);
    let dictionary = &router.engine.extra_dictionaries()[0];
    assert!(dictionary.lookup(&["he", "tong", "fa"], false).is_empty());
    assert_eq!(
        dictionary.lookup(&["min", "fa", "dian"], false)[0].text,
        "民法典"
    );

    // YAML 头中只有引用、没有本文件词条时，失败不能覆盖已有词库。
    std::fs::write(&source, "---\nname: law\nimport_tables:\n  - other\n...\n").unwrap();
    assert!(import::import(&source, &dir.join("dicts")).is_err());
    assert_eq!(Dictionary::from_path(&imported.path).unwrap().len(), 2);

    let removed = dir.join("dicts/removed");
    std::fs::create_dir_all(&removed).unwrap();
    std::fs::rename(&imported.path, removed.join("law.qj")).unwrap();
    poll(&mut router);
    assert!(router.engine.extra_dictionaries().is_empty());
    assert_eq!(std::fs::read(&config_path).unwrap(), original_config);
    drop(router);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn dictionary_changes_do_not_retry_broken_config() {
    let dir = std::env::temp_dir().join(format!("qingjian-reload-broken-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");
    std::fs::write(&config_path, "[general]\npage_size = 5\n").unwrap();
    modified_at(&config_path, 100);
    let config = Config::load(&config_path).unwrap();
    let mut router = Router::new(
        Engine::new(Dictionary::default()),
        RouterConfig::from(&config),
    );
    router.watch_config(
        &config,
        config_path.clone(),
        dir.clone(),
        DataDirs {
            user_root: Some(dir.clone()),
            user_dicts: Some(dir.join("dicts")),
            ..DataDirs::default()
        },
    );

    std::fs::write(&config_path, "[broken").unwrap();
    modified_at(&config_path, 200);
    poll(&mut router);
    let source = dir.join("law.dict.yaml");
    std::fs::write(&source, "---\nname: law\n...\n合同法\the tong fa\t120\n").unwrap();
    let imported = import::import(&source, &dir.join("dicts")).unwrap();
    poll(&mut router);
    assert_eq!(router.config.page_size, 5);
    assert_eq!(router.engine.extra_dictionaries().len(), 1);

    // 保持坏配置的 mtime，以可观察的配置值确认后续轮询不会重新读它。
    std::fs::write(&config_path, "[general]\npage_size = 9\n").unwrap();
    modified_at(&config_path, 200);
    poll(&mut router);
    assert_eq!(router.config.page_size, 5);
    let removed = dir.join("dicts/removed");
    std::fs::create_dir_all(&removed).unwrap();
    std::fs::rename(&imported.path, removed.join("law.qj")).unwrap();
    poll(&mut router);
    assert_eq!(router.config.page_size, 5);
    assert!(router.engine.extra_dictionaries().is_empty());

    modified_at(&config_path, 300);
    poll(&mut router);
    assert_eq!(router.config.page_size, 9);
    drop(router);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// 设置页请求删掉的个人词：Server 处理请求文件、`forget` 完**立刻落盘**，再把请求文件清掉。
/// 设置程序不直接改 `user-words.tsv` 是有原因的 —— 学习数据在 Server 内存里是权威、每 60 秒才落一次盘，
/// 直接改会被覆盖回去（见 `qingjian_platform::dirs::forget_requests_path` 的注释）。
#[test]
fn forget_requests_drop_personal_words() {
    let dir = std::env::temp_dir().join(format!("qingjian-forget-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");
    std::fs::write(&config_path, "").unwrap();
    let config = Config::load(&config_path).unwrap();
    // 个人词表里先躺着一个「合同法」（词表与词频文件同目录，`from_path` 按 sibling 读它）。
    std::fs::write(
        dir.join("user-words.tsv"),
        "# 青简用户词：词\t拼音\t词频，与主词库同格式\n合同法\the tong fa\t10000\n",
    )
    .unwrap();
    let learner = qingjian_learning::FrequencyLearner::from_path(dir.join("user.tsv")).unwrap();
    let engine = Engine::new(Dictionary::default()).with_learner(Box::new(learner));
    let mut router = Router::new(engine, RouterConfig::default());
    router.watch_config(&config, config_path, dir.clone(), DataDirs {
            user_root: Some(dir.clone()),
            ..DataDirs::default()
        },);

    let requests = qingjian_platform::dirs::forget_requests_path(&dir);
    // 首行带 UTF-8 BOM（别的工具写这个文件时会加）：BOM 不能把第一个词带歪，否则「删不掉但文件被清掉」。
    std::fs::write(&requests, "\u{feff}合同法\n").unwrap();
    poll(&mut router);

    assert!(!requests.exists(), "请求文件该被处理掉");
    let words = std::fs::read_to_string(dir.join("user-words.tsv")).unwrap();
    assert!(!words.contains("合同法"), "个人词该被删掉，实际：{words}");
    drop(router);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 个人英文词也算「个人词」：中文模式原样上屏一串字母会被记成英文词（`user-english.tsv`），
/// 设置页的删除请求要能清掉它（2026-09-18 用户反馈 `javashiyimenbianchengyuyan` 删不掉）。
#[test]
fn forget_requests_drop_personal_english_words() {
    let dir = std::env::temp_dir().join(format!("qingjian-forget-en-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");
    std::fs::write(&config_path, "").unwrap();
    let config = Config::load(&config_path).unwrap();
    std::fs::write(
        dir.join("user-english.tsv"),
        "# 青简个人英文词：词\t次数\njavashiyimenbianchengyuyan\t5\nkeep\t2\n",
    )
    .unwrap();
    let learner = qingjian_learning::FrequencyLearner::from_path(dir.join("user.tsv")).unwrap();
    let engine = Engine::new(Dictionary::default()).with_learner(Box::new(learner));
    let mut router = Router::new(engine, RouterConfig::default());
    router.watch_config(&config, config_path, dir.clone(), DataDirs {
            user_root: Some(dir.clone()),
            ..DataDirs::default()
        },);

    let requests = qingjian_platform::dirs::forget_requests_path(&dir);
    std::fs::write(&requests, "javashiyimenbianchengyuyan\n").unwrap();
    poll(&mut router);

    assert!(!requests.exists(), "请求文件该被处理掉");
    let words = std::fs::read_to_string(dir.join("user-english.tsv")).unwrap();
    assert!(
        !words.contains("javashiyimenbianchengyuyan"),
        "个人英文词该被删掉，实际：{words}"
    );
    assert!(words.contains("keep"), "别的英文词不受影响，实际：{words}");
    drop(router);
    let _ = std::fs::remove_dir_all(&dir);
}
