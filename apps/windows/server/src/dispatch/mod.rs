//! 协议分派：把 DLL 发来的 [`ClientMessage`] 交给 Engine，产出回给 DLL 的 [`ServerMessage`]。
//! 消息分派在 [`message`]，会话在 [`session`]，组句展示状态在 [`composed`]，按键在 [`key`]，
//! 候选窗口输出在 [`candidates`]，状态条在 [`status`]，翻译选中文字在 [`translate`]，配置热加载在 [`reload`]，
//! 本地整句模型在 [`rescore`]。

mod candidates;
mod composed;
mod config;
mod key;
mod message;
mod reload;
mod rescore;
mod session;
mod status;
mod translate;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use qingjian_core::Engine;
use qingjian_platform::LocalModelConfig;
use qingjian_platform::protocol::{ClientMessage, Frame, ScreenRect, ServerMessage, SessionId};
use qingjian_predict::PredictConfig;

pub use self::candidates::{CandidateEvent, CandidateSink, NoopSink, RenderSettings};
use self::composed::Composed;
pub use self::config::RouterConfig;
use self::reload::ConfigReload;
pub use self::reload::attach_cloud;
pub use self::rescore::find_model;
use self::rescore::{ModelLoader, RescoreState};
use self::session::SessionInfo;
pub use self::status::{NoopStatusSink, StatusEvent, StatusSink, StatusView};
use self::translate::Translation;

/// 学习数据落盘间隔（与 macOS 壳一致）；Server 没有定时器，借消息节拍看时间。
const LEARNING_FLUSH_INTERVAL: Duration = Duration::from_secs(60);

/// 整句请求最长等多久就不再显示「联想中」（云端那边超时是 5 秒，再留点余量）。
const SENTENCE_PENDING_TIMEOUT: Duration = Duration::from_secs(6);

/// 同一时刻只有一个应用有键盘焦点，所以一个 Engine 持当前组句；焦点切到别的会话时先清掉上一个的残留。
pub struct Router {
    /// 输入内核，进程内唯一。
    engine: Engine,

    /// 每页候选数 / 云端槽位 / 排布 / 外观 / 翻页键等。
    config: RouterConfig,

    /// 活跃会话及各自的宿主应用。
    sessions: HashMap<SessionId, SessionInfo>,

    /// 当前持有组句的会话。
    focused: Option<SessionId>,

    /// 当前组句的展示状态；没在组句时为 `None`。
    composed: Option<Composed>,

    /// 「翻译选中文字」进行态；与 `composed` 互斥。
    translation: Option<Translation>,

    /// 已发出、等 DLL 回选区的请求号；对不上的 `Selection` 丢弃。
    pending_selection: Option<u64>,

    /// 「翻译选中文字」请求号计数器。
    selection_seq: u64,

    /// 整句补全（preedit 右侧、Tab 上屏）；缓冲变化时清空。
    sentence: Option<String>,

    /// 用户这轮是否主动请过整句（按 Tab / 点整句那块）。策略是「按 Tab 才联想」时，
    /// 只有请过之后回来的句子才自动采用，免得模型不守指令时就悄悄上了屏。缓冲一变即清。
    sentence_requested: bool,

    /// 候选窗上点选、还没被 DLL 取走的上屏文本：传输一问一答、Server 不能主动推，只能攒到下一次轮询带回。
    pending_commit: Option<String>,

    /// 整句请求发出去了、结果还没到（记发出时刻）：候选窗在那块位置显示 `☁ …`。
    /// 结果到了 / 组句结束 / 等超（[`SENTENCE_PENDING_TIMEOUT`]）就清，别让它挂着。
    sentence_pending: Option<Instant>,

    /// 删候选后的屏幕提示，随下一帧下发、下一次按键清。
    notice: Option<String>,

    /// 当前高亮候选在布局里的下标（跨页）。
    highlight: usize,

    /// 这轮查询里动过高亮：英文模式空格只在动过之后才选高亮词。
    navigated: bool,

    /// 上次把学习数据落盘的时间。
    last_flush: Instant,

    /// 配置热加载状态；`None` 表示不热加载。
    reload: Option<ConfigReload>,

    /// 候选窗口输出端；Windows 上由 [`crate::ui`] 注入。
    candidates: Box<dyn CandidateSink>,

    /// 悬浮状态条输出端；Windows 上由 [`crate::ui`] 注入。
    status: Box<dyn StatusSink>,

    /// 状态条要显示的中英模式；`None` 表示青简没在前台（还没有会话报过模式 / 切成了别的输入法），不显示。
    /// 应用退出不影响它：状态条是桌面常驻的，只跟「当前输入法是不是青简」走。
    status_mode: Option<bool>,

    /// 状态条上点出来、还没被 DLL 用 `SyncMode` 取走的目标模式。
    pending_mode: Option<bool>,

    /// 云联想配置：「☁」格翻转 `enabled` 后按它换 Predictor，热加载时跟着 `[predict]` 走。
    predict: PredictConfig,

    /// 聚焦会话最近报来的光标矩形；云联想异步到达时按它原地重摆候选窗口。
    last_rect: Option<ScreenRect>,

    /// 上次真正显示的帧与位置：没变就不重画（组字期间的空转 Poll 很多）。
    last_shown: Option<(Frame, ScreenRect)>,

    /// 本地整句模型（`.qjm` 或三件套目录）；没有模型文件为 `None`。
    model_path: Option<PathBuf>,

    /// 进行中的模型加载；加载完接到 Engine 上就清掉。
    model_loader: Option<ModelLoader>,

    /// 上次套用的 `[model]`，变了才重载 / 卸载。
    applied_model: LocalModelConfig,

    /// 重排的防抖 / 轮询进行态。
    rescore: RescoreState,
}

impl Router {
    pub fn new(engine: Engine, config: RouterConfig) -> Self {
        Self {
            engine,
            config: RouterConfig {
                page_size: config.page_size.max(1),
                ..config
            },
            sessions: HashMap::new(),
            focused: None,
            composed: None,
            translation: None,
            pending_selection: None,
            selection_seq: 0,
            sentence: None,
            sentence_requested: false,
            pending_commit: None,
            sentence_pending: None,
            notice: None,
            highlight: 0,
            navigated: false,
            last_flush: Instant::now(),
            reload: None,
            candidates: Box::new(NoopSink),
            status: Box::new(NoopStatusSink),
            status_mode: None,
            pending_mode: None,
            predict: PredictConfig::default(),
            last_rect: None,
            last_shown: None,
            model_path: None,
            model_loader: None,
            applied_model: LocalModelConfig::default(),
            rescore: RescoreState::default(),
        }
    }

    pub fn set_candidate_sink(&mut self, sink: Box<dyn CandidateSink>) {
        sink.configure(self.config.render_settings());
        self.candidates = sink;
    }

    /// 直接碰 Engine：测试里改模式键这类启动时才设的开关。
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// 启动时记下云联想配置（缺省关）；「☁」格按它翻转 `enabled` 并当场重新接入。
    pub fn configure_predict(&mut self, predict: PredictConfig) {
        self.predict = predict;
    }

    pub fn set_status_sink(&mut self, sink: Box<dyn StatusSink>) {
        self.status = sink;
    }

    /// 处理一条消息；`None` 表示不用回话。到点顺带把学习数据落盘。
    pub fn handle(&mut self, message: ClientMessage) -> Option<ServerMessage> {
        let response = self.dispatch(message);
        if self.last_flush.elapsed() >= LEARNING_FLUSH_INTERVAL {
            self.flush_learning();
        }
        response
    }

    pub fn flush_learning(&mut self) {
        self.engine.flush_learning();
        self.last_flush = Instant::now();
    }
}
