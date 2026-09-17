use qingjian_core::PredictionPolicy;
use serde::{Deserialize, Serialize};

/// `lookback` 的上限：光标前最多能配多少字。
/// Windows 的 TSF 侧就按它读光标前的文本——读得再多也没用，真正发出去多少由 `lookback` 决定。
pub const MAX_LOOKBACK: usize = 512;

/// `lookahead` 的上限：光标后最多能配多少字，同理。
pub const MAX_LOOKAHEAD: usize = 256;

/// 云联想配置。默认**关闭**，开启后光标附近的文本会发往 `base_url`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PredictConfig {
    /// 是否启用。
    pub enabled: bool,

    /// OpenAI 兼容接口地址（不含 `/chat/completions`）。
    pub base_url: String,

    /// 模型名。
    pub model: String,

    /// 密钥。留空则读 `api_key_env` 指定的环境变量。
    pub api_key: Option<String>,

    /// 存放密钥的环境变量名。
    pub api_key_env: String,

    /// 单次请求超时（毫秒），超时即丢。
    pub timeout_ms: u64,

    /// 防抖：停止敲键多久之后才真正发请求（毫秒）。
    pub debounce_ms: u64,

    /// 光标前最多发多少个字符。
    pub lookback: usize,

    /// 光标后最多发多少个字符。
    pub lookahead: usize,
    /// 云端词最多补进候选窗口第一页末尾几格；0 表示不要云端词，只要整句补全。
    pub slots: usize,

    /// 组句中要不要整句补全。
    pub sentence: bool,

    /// 整句补全什么时候要：`idle` 停止输入后自动联想（配合 `debounce_ms`，与云端词同一趟请求），
    /// `tab` 只在用户按 Tab / 点整句那块时现请一次。云端词不受它影响，两种模式下都照常自动联想。
    pub sentence_trigger: SentenceTrigger,

    /// 推理强度，随请求发 `reasoning_effort`：`none` 关掉模型的思考（联想要的是快，不是想），
    /// 其余 minimal / low / medium / high / xhigh 照传；留空则不发（给不认这个参数的接口）。
    /// DeepSeek V4 这类默认带思考的模型不关会把 token 预算全花在思考上，正文为空。
    pub reasoning_effort: String,
}

impl Default for PredictConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: "https://api.deepseek.com".to_owned(),
            model: "deepseek-v4-flash".to_owned(),
            api_key: None,
            api_key_env: "QINGJIAN_API_KEY".to_owned(),
            timeout_ms: 5000,
            debounce_ms: 300,
            lookback: 64,
            lookahead: 32,
            slots: 2,
            sentence: true,
            sentence_trigger: SentenceTrigger::default(),
            reasoning_effort: "none".to_owned(),
        }
    }
}

impl PredictConfig {
    /// 配置里的密钥优先，其次环境变量；两边都没有返回 `None`。
    pub fn resolve_api_key(&self) -> Option<String> {
        self.api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(str::to_owned)
            .or_else(|| std::env::var(&self.api_key_env).ok())
            .filter(|k| !k.trim().is_empty())
    }

    pub fn policy(&self) -> PredictionPolicy {
        PredictionPolicy {
            before: self.lookback,
            after: self.lookahead,
            slots: self.slots,
            // 比槽位多要两条，与本地候选重复的去掉后还能填满；问字模式的答案也按这个数要
            max_items: self.slots.max(1) + 2,
            // 「按 Tab 才联想」时自动那趟不问整句：用户按 Tab / 点整句时才现请（`Engine::request_sentence_once`）。
            sentence: self.sentence && self.sentence_trigger == SentenceTrigger::Idle,
        }
    }

    /// 整句补全是不是「开关开着、但不自动问，等用户按 Tab / 点一下才问」——壳据此决定要不要显示那块按钮。
    pub fn sentence_on_tab(&self) -> bool {
        self.sentence && self.sentence_trigger == SentenceTrigger::Tab
    }
}

impl SentenceTrigger {
    /// 配置里的写法，与序列化名一致（设置页按它选中当前项）。
    pub fn key(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Tab => "tab",
        }
    }
}

/// 整句补全的触发方式（配置 `[predict] sentence_trigger`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SentenceTrigger {
    /// 停止输入后自动联想（缺省，与云端词同一趟请求）。
    #[default]
    Idle,

    /// 只在按 Tab / 点整句那块时要。
    Tab,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 缺省：停止输入后自动要整句，`sentence_on_tab` 为假。
    #[test]
    fn idle_trigger_asks_the_sentence_on_every_request() {
        let config = PredictConfig {
            enabled: true,
            ..PredictConfig::default()
        };
        assert_eq!(config.sentence_trigger, SentenceTrigger::Idle);
        assert!(config.policy().sentence);
        assert!(!config.sentence_on_tab());
    }

    /// 按 Tab：自动那趟不问整句（省得模型不守指令就自动上屏），但壳要认得出这模式能现请。
    #[test]
    fn tab_trigger_keeps_the_auto_request_words_only() {
        let config = PredictConfig {
            enabled: true,
            sentence_trigger: SentenceTrigger::Tab,
            ..PredictConfig::default()
        };
        assert!(!config.policy().sentence, "自动请求只问云端词");
        assert!(config.sentence_on_tab());
    }

    /// 整句补全整个功能关掉时，两种触发方式都不该要句子。
    #[test]
    fn sentence_off_outranks_the_trigger() {
        let config = PredictConfig {
            enabled: true,
            sentence: false,
            sentence_trigger: SentenceTrigger::Tab,
            ..PredictConfig::default()
        };
        assert!(!config.policy().sentence);
        assert!(!config.sentence_on_tab(), "功能都关了，Tab 也不该去现请");
    }
}
