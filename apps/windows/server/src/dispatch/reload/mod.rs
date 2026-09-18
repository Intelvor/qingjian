//! 配置热加载：空闲时看 `config.toml` 的 mtime，改了就重读并应用（与 macOS 壳对齐）。
//! 便宜的设置无条件重设；云联想 / 释义表按配置变化重建，附加词库也检查文件增删与更新。热加载状态在 [`ConfigReload`]。

mod state;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use qingjian_core::{Engine, Language, NoGlossFiller, NoPredictor, NoTranslator};
use qingjian_platform::{Config, extra_dictionaries};
use qingjian_predict::{CloudGlossFiller, CloudPredictor, PredictConfig};

pub(super) use self::state::ConfigReload;

/// 看配置文件 mtime 的最短间隔；工人循环空闲时按它等，重排的短节拍来得更勤时按这个节流。
pub(super) const CONFIG_POLL_INTERVAL: Duration = Duration::from_secs(1);
use super::{Router, RouterConfig};
use crate::assembly::{self, user_dicts_dir};

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

/// 按 `[predict]` 接云联想与释义兜底；关着或缺密钥就退回本地实现。启动与热加载共用。
pub fn attach_cloud(engine: &mut Engine, predict: &PredictConfig) {
    if !predict.enabled {
        tracing::info!("云联想未开启（[predict] enabled = false）");
        engine.set_predictor(Box::new(NoPredictor));
        engine.set_gloss_filler(Box::new(NoGlossFiller));
        return;
    }
    match CloudPredictor::new(predict) {
        Ok(predictor) => {
            engine.set_predictor(Box::new(predictor));
            tracing::info!(model = %predict.model, "云联想已接入");
        }
        Err(error) => {
            tracing::warn!(%error, "云联想接入失败（缺 API key？），退回本地候选");
            engine.set_predictor(Box::new(NoPredictor));
        }
    }
    match CloudGlossFiller::new(predict) {
        Ok(filler) => engine.set_gloss_filler(Box::new(filler)),
        Err(error) => {
            tracing::warn!(%error, "释义兜底未启用");
            engine.set_gloss_filler(Box::new(NoGlossFiller));
        }
    }
}

/// 学习语言变了就换释义表：关是不翻译；换语言重装随包 + 个人释义表，没有这门语言的表或装不上就保持原样。
/// 换成功（或关掉）返回 true。
fn swap_translator(
    engine: &mut Engine,
    language: Option<Language>,
    root: &Path,
    user_dir: Option<&Path>,
) -> bool {
    let Some(language) = language else {
        engine.set_translator(Box::new(NoTranslator));
        tracing::info!("学习语言已关，不显示译文");
        return true;
    };
    let Some(path) = assembly::glossary_file(root, language) else {
        tracing::warn!(
            language = language.code(),
            "没有这门语言的释义表，学习语言不变"
        );
        return false;
    };
    match assembly::load_glossary(language, &path, user_dir) {
        Ok(glossary) => {
            tracing::info!(language = language.code(), "释义表已切换");
            engine.set_translator(Box::new(glossary));
            true
        }
        Err(error) => {
            tracing::warn!(%error, "释义表加载失败，学习语言不变");
            false
        }
    }
}

impl Router {
    /// `config.toml` 路径；没开热加载（测试）时为 `None`。
    pub(super) fn config_path(&self) -> Option<&Path> {
        self.reload
            .as_ref()
            .map(|reload| reload.config_path.as_path())
    }

    /// 开启热加载：记下路径与当前已应用的 predict / dictionaries / 学习语言。
    pub fn watch_config(
        &mut self,
        config: &Config,
        config_path: PathBuf,
        root: PathBuf,
        user_dir: Option<PathBuf>,
    ) {
        let last_mtime = mtime(&config_path);
        let dictionary_files = user_dicts_dir(user_dir.as_deref())
            .map(|dir| extra_dictionaries::snapshot(&dir))
            .unwrap_or_default();
        let bundled_dicts_dir = Some(root.join("data/generated/dicts")).filter(|dir| dir.is_dir());
        self.reload = Some(ConfigReload {
            config_path,
            last_check: Instant::now(),
            root,
            bundled_dicts_dir,
            user_dir,
            last_mtime,
            applied_predict: config.predict.clone(),
            applied_dictionaries: config.dictionaries.clone(),
            dictionary_files,
            applied_language: assembly::learning_language(config),
        });
    }

    /// 空闲时调；一秒内只真正看一次文件。解析失败保持原配置，mtime 照记（不每秒重试同一个坏文件）。
    pub fn poll_config_reload(&mut self) {
        // 「该不该看」与 `user_dir` 先取出来：下面处理删除请求要 `&mut self`，`reload` 的借用得先放下。
        let user_dir = {
            let Some(reload) = &mut self.reload else {
                return;
            };
            if reload.last_check.elapsed() < CONFIG_POLL_INTERVAL {
                return;
            }
            reload.last_check = Instant::now();
            reload.user_dir.clone()
        };
        // 设置页请求删掉的个人词：它读不到我们内存里那份学习数据，所以只投个请求过来，
        // 由我们 `forget` 完立刻落盘 —— 直接改 `user-words.tsv` 会被下一次落盘（60 秒一次）覆盖回去。
        self.apply_forget_requests(user_dir.as_deref());
        let Some(reload) = &mut self.reload else {
            return;
        };
        let files = user_dicts_dir(reload.user_dir.as_deref())
            .map(|dir| extra_dictionaries::snapshot(&dir))
            .unwrap_or_default();
        let dictionaries_changed = files != reload.dictionary_files;
        if dictionaries_changed {
            // 配置损坏也继续使用上次有效的词库开关；文件变化不触发配置重试。
            self.engine
                .set_extra_dictionaries(reload.load_dictionaries());
            reload.dictionary_files = files;
        }
        let current = mtime(&reload.config_path);
        if current == reload.last_mtime {
            return;
        }
        reload.last_mtime = current;
        let path = reload.config_path.clone();
        match Config::load(&path) {
            Ok(config) => {
                self.apply_config(&config);
                tracing::info!("配置已热加载");
            }
            Err(error) => tracing::error!(%error, "配置热加载解析失败，保持原配置"),
        }
    }

    /// 处理设置页投过来的「删掉这些个人词」请求：逐个 [`Engine::forget_word`]，然后**立刻落盘**
    /// （`flush_learning`），最后把请求文件删掉。幂等：删不掉（文件被占）时留着，下一拍再试。
    fn apply_forget_requests(&mut self, user_dir: Option<&Path>) {
        let Some(path) = user_dir.map(qingjian_platform::dirs::forget_requests_path) else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        let words: Vec<&str> = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        if words.is_empty() {
            let _ = std::fs::remove_file(&path);
            return;
        }
        let mut removed = 0;
        for word in &words {
            if !self.engine.forget_word(word).is_nothing() {
                removed += 1;
            }
        }
        // 落盘把删除写进 `user-words.tsv`：不落的话设置页再读文件还是能看见它。
        self.flush_learning();
        match std::fs::remove_file(&path) {
            Ok(()) => tracing::info!(asked = words.len(), removed, "按设置页的请求删掉个人词"),
            Err(error) => {
                tracing::warn!(%error, asked = words.len(), removed, "删掉个人词后清不掉请求文件")
            }
        }
    }

    /// 应用新配置。学习语言变了换释义表（词汇等级表启动时已全装，不用换）。
    fn apply_config(&mut self, config: &Config) {
        self.engine.set_fuzzy(config.fuzzy);
        self.engine.set_shuangpin(config.general.shuangpin());
        self.engine.set_zhuyin_mode(config.general.zhuyin);
        self.engine.set_traditional_mode(config.general.traditional);
        self.engine.set_learning(config.general.learning);
        self.engine.set_mode_keys(config.shortcut.mode);
        self.engine.set_chinese_first(config.general.chinese_first);
        self.engine
            .set_shift_letter_compose(config.general.shift_letter.compose());
        let previous = self.config.render_settings();
        self.config = RouterConfig::from(config);
        // 「☁」格显示开不开、点下去往哪边翻，都跟配置文件走，两边不会各说各话。
        self.predict = config.predict.clone();
        let settings = self.config.render_settings();
        if settings != previous {
            self.candidates.configure(settings);
        }
        self.reconcile_status();
        self.apply_model_config(&config.model);

        let Some(reload) = &mut self.reload else {
            return;
        };
        let mut predict_changed = false;
        if config.predict != reload.applied_predict {
            attach_cloud(&mut self.engine, &config.predict);
            reload.applied_predict = config.predict.clone();
            predict_changed = true;
        }
        let language = assembly::learning_language(config);
        if language != reload.applied_language
            && swap_translator(
                &mut self.engine,
                language,
                &reload.root,
                reload.user_dir.as_deref(),
            )
        {
            reload.applied_language = language;
        }
        if config.dictionaries != reload.applied_dictionaries {
            reload.applied_dictionaries = config.dictionaries.clone();
            self.engine
                .set_extra_dictionaries(reload.load_dictionaries());
        }
        // 换了（或关了）Predictor：手里那份整句作废，别让 Tab 或点选上屏旧句子。
        // 放在 `reload` 借用结束之后，两条借用才不打架。
        if predict_changed {
            self.drop_sentence();
        }
    }
}
