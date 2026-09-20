# 各 crate 的实现要点

CLAUDE.md 只保留目录地图与规则，每个 crate / app / tool 的实现细节收在这里：入口类型、数据文件、常数、生成命令。
改了实现要同步改这里；与代码冲突时以代码为准。

## crates/qingjian-dictionary

词库（TSV 解析或 `.qj` mmap），键按字节序排好，查询逐音节位置二分收窄（简拼位置按音节块跳扫），
`lookup_pattern`（≥ 模式长度）与 `lookup_exact`（正好等长）同一套实现。词库键以 `v` 表示 ü，
TSV 解析、查询与生成工具把 `lue` / `nue` 统一成 `lve` / `nve`。
旧 `.qj` 含这些键时，加载器建立规范化的内存词库。新 `.qj` 继续使用 mmap。

`CodeTable` 是形码码表（五笔），与词库并列的另一类查表：键是编码本身（`ggll`），不是拼音音节序列，
格式 `词\t编码\t词频`（`assets/wubi/wubi86.tsv`，8.9 万条），按 `(编码, 词频降序)` 排好、`lookup` 二分定位前缀区间。
编码打全的词（`exact`）排在同前缀的更长编码词前面，这就是一级 / 二级简码的取法，不需要另做简码表。
与词库的一处关键差别：码表没有 `.qj` 容器，只读 TSV。

## crates/qingjian-core

模块：`composition`（缓冲区与光标；中文模式下 Shift+字母按小写进 `buffer` 参与匹配、大写记在 `shifted`，`typed_text` 还原后用于原样上屏）/ `parser` / `correction`（拼写纠错：整段一处编辑的候选纠正 + `typo` 音节级敲错变体表，后者进整句词图当带代价的边）/
`candidate` / `ranking` / `shortcut` / `sentence` / `fuzzy` / `shuangpin`（双拼：七套方案键位表、键 → 全拼解码与消耗换算）/ `zhuyin`（大千注音：键 → 注音符号 → 拼音，`[general] zhuyin` 开关，声调只判音节完整不进查询）/ `emoji` /
`english`（英文模式候选）/ `engine`（`query::EnglishTail`：句末英文词并入整句，`woxiangxuehaorust` → 我想学好rust，尾段也像拼音时按分数与拼音读法比）。
形码（五笔）在 `engine::query::code`。`Engine` 上有两个开关：`set_code_table`（码表）与 `set_phonetic`（拼音侧参不参与），
在 `query_inner` 进切分之前按这两个分派——只有拼音 / 只有形码（`query_code`）/ **两边都开（`query_mixed`，混输）**。
编码按前缀查表，`CandidateKind::Code` 的候选 `syllables` 为空、上屏吃掉整段作用域（`whole_scope`）。
混输是「拼音那条 Query 前面插上形码候选」：拼音读不出来时（`ggll`）整个按形码走，两边都空才算错；
按文本去重，编码打全的形码词在前、拼音居中、只命中前缀的形码词垫后（一律形码在前的话 `kai` 的首选会变成编码 `kaik` 的词）；混输下模式键同双拼换成大写，四码以内不做拼写纠错，且五笔码最长 4 位、第 5 个字母起自然只剩拼音。
拼音那套在纯形码下全部不适用，靠 `modes()` 返回 `ModeKeys::LETTERLESS` 与 `active_correction` 直接返回 `None` 关掉；
译词标注、生词记录、输入日志、用户选择学习与个人 n-gram 仍照常工作。
`Engine` 是对外唯一门面，`Translator` / `Learner` trait 在 `engine` 模块；词库是「主词库 + 附加词库（`set_extra_dictionaries`）+ 用户词」的列表；繁体输出（`traditional` 开关与 `traditional_map` 映射）依赖 `ferrous-opencc`（`s2tw`）在出候选与上屏边界转换，内部保持简体。
`Engine` 是对外唯一门面，`Translator` / `Learner` trait 在 `engine` 模块；词库是「主词库 + 附加词库（`set_extra_dictionaries`）+ 用户词」的列表。
- 中英混输的英文词位置：`Engine::set_chinese_first`（配置 `[general] chinese_first`，缺省关）关着时拼音不像话的输入英文排第一（`extras::insert_english`，
  用户老选中文词时仍让中文在前），开着时整句先插、英文词紧随其后排第二（`query_inner` 里两步的先后按开关掉转）；句末英文词并入整句（`EnglishTail`）不受它影响。
  缺省关是回放定的（9241 词 / 269 条英文上屏：缺省开英文首选 82.5% → 7.1%）。
- `custom_phrase::merge_replacements` 把平台给的「输入码 → 短语」表（macOS 系统文本替换）并进配置里的自定义短语：每条占该码最靠前的空位（1–9），
  输入码不是小写字母、已有同码同文本、九位都满的跳过；Core 不管数据从哪来。
- 拼写纠错（`correction`）：整段一处编辑的变体里相邻换位允许末尾音节没敲完（`loose_segmentation`，`mignt` → `ming t…`；
  有「每个音节都完整、末尾不是落单单字母」的变体时残尾变体不参与，免得 `keyyi` 的 可以 被 `ke yi y…` 抢走），
  纠正之间比较时换位减 `TypoCosts::correction_transpose_discount`（1 nat，CLI `--tune correction-transpose=`），与原样比不减；
  2026-09-15 回放 283 词 / 23 句：词首选 89.0%、整句 82.6%，与改前持平（折扣若也用在与原样比，`zhongwne` 会误纠成 中文，整句掉一条）。

`EngineSession` 保存可挂起的组句、标点、历史与学习链，`Engine::swap_session` 在同一个引擎里交换输入状态，共用词库与落盘服务。切换上下文时清除查询及异步预测缓存，并由平台恢复各自私密状态。

`Engine::discard_input` / `EngineSession::discard_input` 用于隐私能力变化时无痕清理输入，包括透传缓冲、学习链和暂存词汇曝光；`set_private` 只切换写入开关，保留已输入的组句。

## crates/qingjian-translate

`Glossary`，本地 TSV 释义表（词性 + 译文）；`LevelTable`，词汇等级表（`assets/levels/levels-{en,ja}.tsv`，CEFR A1–C2 / JLPT N5–N1，
`uv run tools/corpus/levels.py` 从 `data/levels/` 的原始 CSV 生成，来源与许可见 `assets/levels/README.md`），「统计」页按级数词汇用，不进候选。

## crates/qingjian-learning

- `FrequencyLearner`：用户选择次数（`user.tsv`）、按输入串记的选择（`user-choices.tsv`，词级排序里同输入串选过的优先）、用户词（`user-words.tsv`，主词库同格式，
  Engine 与主词库一起查）、个人英文词（`user-english.tsv`，回车原样上屏的英文词与选过的英文候选，与随包英文词表一起出候选且在前）、
  个人敲错表（`user-typos.tsv`，接受过的 (敲的, 要的) 音节对，词图敲错边与整段纠错的代价按它打折）与个人 n-gram（`user-ngram.tsv`，Core `sentence::UserNgram`，
  二元 + 三元在线计数，整句转换与词级排序里与静态模型插值；Tab 接受的云端整句按 `sentence::segment_text` 切词后也记；
  连着选出的两个词记够次数自动造词进用户词，一段拼音分几次选完的合成词记两次也造）。
  **`forget` 要连个人英文词一起清**：中文模式下把一串字母原样上屏也会记成「英文词」（`learn_english` → `user-english.tsv`），
  它不在用户词表里，漏掉就「删了跟没删一样」（2026-09-18 用户反馈 `javashiyimenbianchengyuyan` 被记住却找不到）。
  设置页「词库 → 个人词」把 **`user-words.tsv` 与 `user-english.tsv` 合并列出**（英文词那列写「个人英文词 · 上屏过 N 次」），
  删除走同一条请求文件；读 `forget-requests.txt` 时要**去掉首行 BOM**（别的工具写它会带 BOM，BOM 会让第一个词匹配不上）。
- `InputLog`：输入日志（`input-log.jsonl`，每次上屏一行：敲的键、切分、看到的前几个候选、选了第几个、来源、纠错、撤销，
  Core `InputLogger` trait 的落盘实现，`[general] input_log` 缺省开，只写本机，给离线回归评测与个人模型用）。
- `UsageStats`：输入统计（`usage.tsv`，按天记汉字 / 中文词 / 英文词 / 上屏次数，Core `UsageMeter` trait 的实现，Engine 每次上屏 `Usage::of_text` + 按来源定词数，
  整句按 `segment_text` 切词数；与输入日志无关，偏好设置「统计」页显示，`book_scale` 折成几本《某书》）。
- `VocabularyBook`：词汇记录（`user-vocab.tsv`，Core `VocabularyTracker` trait 的实现：学习语言的每条译词看到过几轮 / 上屏过 / ⌥+数字 打出过几次；Core 私密输入统一跳过曝光和提交写入，但仍可读取已有记录用于排序和生词标记；
  Engine `annotate` 据此填 `Sense::fresh`，看到轮次不到 `FRESH_UNTIL` = 3 的译词壳里画橙色；「看到」按上屏那一刻屏幕上那一页算，壳每次画完 `Engine::note_displayed` 告知当前页）。
- 各表落盘走 Core `storage::write_atomic`（临时文件 + fsync + 改名），加载按行容错（坏行警告跳过，真读不了壳退回内存学习），
  壳激活期间每 60 秒 `Engine::flush_learning`；IMK 回调边界 `imk::catch_panic` 拦 panic、缓冲区字母原样上屏（见 architecture.md「崩溃不丢」）。

## crates/qingjian-predict

- `CloudPredictor`：`Predictor` trait 的网络实现（async-openai，OpenAI 兼容接口，默认 DeepSeek），后台线程防抖 / 缓存 / 超时，`submit` / `poll` 非阻塞。
  `PredictConfig` 是配置的 `[predict]` 分节。只在组句中联想，一次请求给云端词（补进候选第一页末尾 `[predict] slots` 格，缺省 2，不预留不占位，
  前面的本地候选不挪；排布在 Core `CandidateLayout`）和整句补全（preedit 右侧，Tab）；上屏后不联想，本地历史不进请求。
  **2026-09-18 改口径：给模型的东西不做任何本地加工，回什么就采纳什么。**
  请求里只有「**用户敲的原始按键** + **候选窗第一页**（最多 9 格，`PREDICTION_CANDIDATE_HINTS`）+ 光标前后文 +
  `scheme`（输入方式：全拼 / 小鹤双拼 / 大千注音 / 五笔（86 版）/ 五笔 + 全拼（混输））+ `traditional`（简繁）+
  `max_items` / `want_sentence`」——不再发本地切分（`pinyin` / `syllables`）、本地整句（`local_sentence`），
  也不要求模型回 `sentence_pinyin`。**`letters` 区分大小写**（2026-09-18）：全拼 / 五笔走 `Composition::typed_scope()`
  （`shift_letter = "compose"` 时 Shift 大写还原进请求，模型据此判专有名词）；双拼 / 注音仍发 `decode` 后的全拼。
  提示词（`qingjian-predict/src/prompt.rs::SYSTEM_PROMPT`）用一段说明 + 六个示例
  把行为框住：忽略简单拼写错误、中英混排照写、**不与候选窗第一页重复**、有 after 时只填中间那段、数字按中文习惯写、
  `letters` 为空是续写、**letters 带大小写时英文专名照抄**。**五笔 / 混输**时提示词会点明 `letters` 是字根编码不是拼音（`scheme` 字段就是为这个）。
  解析（`parse_reply`）只做组装层面的两件清理：剥掉模型重复写进来的 before / after、**不与候选窗第一页重复**；
  云端词与整句都**不再拿拼音校验**（Core 侧的 `validate_cloud_words` 与 `prediction/fuzzy.rs` 已删）。
  不要云端词的情形：`[predict] slots = 0`、简拼（`mostly_abbreviated`），以及**敲到 20 个字母**（`WORD_PREDICTION_MAX_LETTERS`，
  这么长的输入本来就是一整句，只要整句预测；问字模式不受这条限制）。
  双拼 / 注音与拼音同一套：请求前先 `Engine::decode` 把敲的键换成全拼再发（模型看不懂双拼击键），
  `letters` 因此是全拼；简拼判定照旧（cloud.rs 覆盖）。
- **Shift 大写进组句（`shift_letter = "compose"`）不当拼音小写匹配**（`engine/query/shifted.rs`，2026-09-18）：
  先按整段 / 从作用域开头起的前缀拼英文词表（`GitHub`），拼不出就孤立大写字母（`C`）；拼音只吃大写之前的纯小写前缀。
  中途大写留给下一轮（先上屏前面的拼音）。英文候选带音节时按音节消耗上屏。
- `CloudGlossFiller`：释义兜底（Core `GlossFiller` trait，与 Predictor 分开的线程与通道，攒 1.5 秒 / 8 个词发一次，问过不再问）：
  随包释义表没有的词库词 / 云端词上屏后入队，结果壳每秒 `Engine::poll_glosses` 经 `Translator::learn` 写进 `qingjian-translate::PersonalGlossary`
  （`user-glossary-<语言>.tsv`，`LayeredTranslator` 个人表优先）；随云联想开关一起开。
- 问字键（缺省 `u`）开头是问字模式（`PredictionKind::Question`，答案带读音、不校验拼音），`?` 开头要 `ModeKeys::question_mark` 开着才算（配置 `[shortcut] question_mark`，缺省关，壳用 `Engine::takes_question_mark` 决定空缓冲区的 `?` 是入口还是标点）；`PredictionKind::Translate` 是壳里快捷键触发的「翻译选中文字」
  （双向：汉字为主译成学习语言，外文译回中文，`prediction::translation_target`），译文走结果的 `sentence`。

## crates/qingjian-format

`.qj` 数据容器（`Container` mmap 读、`Writer` 写、`Table<T>` / `Text` 零拷贝视图、`hash` 可落盘哈希索引、`Metadata` 名称 / 许可证 / 署名）。
词库与语言模型都能 `write_qj` / 从 `.qj` 打开，启动 50 ms；`cargo run --release -p qingjian-dict-convert -- pack dict|lm --name … --license …`
生成 `data/generated/{dict,lm}.qj`，`bundle.sh` 在 TSV 更新时自动重打并只把 `.qj` 打进包。设计见 `docs/design/architecture.md`「数据文件：`.qj` 容器」。

## crates/qingjian-neural

`CharScorer`，Core `sentence::SentenceScorer` trait 的实现：candle 加载字级 Transformer（GPT-2 风格 decoder，训练仓库（本地 `../train`，私有，不在本仓库）导出的
`model.safetensors` + `config.json` + `vocab.json`），给「前文 + 整句」按字累加 log 概率；前文的每层 K / V 缓存（`PrefixCache`），
同一段前文只算一次，每个候选只算自己那几个字（64 字前文 × 8 条 28 ms，Metal）。features `accelerate` / `metal` 换后端，壳用 `metal`。

Engine 侧在 `engine/rescoring/`：接了打分器就取 Viterbi 前 `RESCORE_PATHS` = 6 条路径按 `路径分 + λ·(神经分 − 静态二元分)` 重排（λ `NEURAL_WEIGHT` 0.5，
个人 n-gram / 用户加分 / 代价不动），分走「前文 + 文本 → 神经分」缓存 `NeuralCache`；同步打分器（`with_sentence_scorer`，CLI 评测）当场补分，
异步的（`with_async_sentence_scorer`，后台线程 `RescoreWorker`）查询不等模型：缺分的记下来，壳停键后 `request_rescoring`、`poll_rescoring` 到了再 `query` 一次。
前文优先用壳给的应用光标前文（`set_rescoring_context`），没有用本会话最近 64 个上屏字符。CLI `--neural <导出目录>`（`--neural-weight` / `--neural-context` / `--neural-async`）。

## crates/qingjian-lm

`BigramModel`，Core `sentence::LanguageModel` trait 的实现，从 `data/generated/lm.qj`（或 `lm-unigram.tsv` / `lm-bigram.tsv`）加载
（没有这两个文件就退化为一元词频整句）。数据由 `tools/corpus/parquet_to_text.py`（uv 脚本，HF parquet → 简体纯文本）加
`cargo run --release -p qingjian-dict-convert -- bigram --phrases assets/lexicon/phrases.tsv --phrases assets/lexicon/domain_words.tsv --brand assets/lexicon/brand.tsv --brand assets/lexicon/mixed_words.tsv data/corpus/*.txt` 生成；语料在 `data/corpus/`（gitignore）。
短语层不当 token 统计（分词时摘掉、统计完按成分合成一元 / 二元，短语得分等于原来两个词的路径，见 `bigram.rs` 模块注释），品牌词按给定次数写进一元与句首二元。

## crates/qingjian-platform

输入方案是**两条独立的轴**：`Scheme`（拼音侧：全拼 / 双拼七套 / 大千注音 / 关，`[general] scheme`）
与 `[general] wubi`（形码侧：空为关 / `wubi86`）。两边都开就是**混输**（`GeneralConfig::mixed`）。
`scheme_label(pinyin, wubi)` 是状态条显示的方案名（形码在前），做成自由函数而不是存进 `RouterConfig`——
存了会与那两项冗余、手搓配置的地方就漂移（状态条那条测试正是这么发现的）。

演进：2026-09-16 之前是 `shuangpin` + `zhuyin` 两个键表达同一个维度，先并成单选的 `scheme`，
同一天又拆成两条轴（单选的 `scheme` 表达不了混输）。旧键（`shuangpin` / `zhuyin` / `scheme = "wubi86"`）
都还在读、文件不自动改写。


`Config`（TOML 配置文件，`[general]` / `[shortcut]` / `[fuzzy]` / `[dictionaries]` / `[apps]` / `[predict]` 分节，首次运行写模板，
`set_value` 用 toml_edit 原地改键保留注释；`[model] enabled` 本地整句模型开关，`LocalModelConfig`）；`extra_dictionaries` 列出 / 加载随包领域词库与用户 `dicts/`
（mac 壳与 Windows Server 共用，同名 `.qj` 优先于 `.tsv`）；`protocol` 模块是 Windows Server ↔ TSF DLL 的 IPC 协议类型
（`ClientMessage` / `ServerMessage` / `Frame` / `PreeditSegment`，全 serde，两端共用，见 `docs/design/architecture.md`「Windows：TSF」）。

## crates/qingjian-render

自绘渲染器：候选窗一帧 + 主题 → 预乘 RGBA 位图，tiny-skia 栅格 + cosmic-text 文字（fontdb 按平台清单只加载几个字体文件、不扫系统），
自己解析 `trak` 字距表、按主题 gamma 加深笔画；cosmic-text 打了 `opsz` 光学字号补丁（qingjian-team/cosmic-text 分支 `qingjian-opsz`，workspace `[patch.crates-io]` 钉 rev）。
`examples/preview.rs` 出 PNG 与真机截图并排比、`--measure` 与 AppKit 对宽度。mac 壳 `candidates/bitmap/` 贴位图，`[general] renderer = "system"` 切回 AppKit 绘制
（过渡期退路，偏好设置「候选窗口」页可选）；`[general] font` 是候选窗字族名、`font_size` 是字号（点，8–32，只对青简渲染器生效；空为系统字体，`bitmap/font_files.rs` 用 CoreText 按字族名找文件只加载那几个，没装就回系统字体；
设置页 `preferences/font_picker/` 是搜索框 + 列表）。设计与验收见 `docs/design/rendering.md`。

## apps/cli

测试工具，`cargo run -p qingjian-cli -- kaifa`。

- `--predict` 强制开云联想并等结果打印，交互模式下上屏后也联想。
- `--wubi <码表>` 用形码码表（`词\t编码\t词频` 的 TSV）替代拼音：按键当编码按前缀查表，候选不带音节，上屏吃掉整段编码。
- `--typing` 逐键计时（性能测试用 release 构建跑，目标每键 10 ms 以内）。
- `--chinese-first` 打开中文优先（`[general] chinese_first = true` 的排法），配合 `--replay` 比两种英文词位置。
- `--replay <input-log.jsonl>` 回放评测：把日志里每次上屏的键重新喂给引擎，按来源算首选 / 前五命中率、平均名次、不在候选的条数，打印没命中的例子（`--misses N`）；
  只在内存里学习不写文件，加 `--user-dict` 可带上现有学习数据。
  **按日志记的方案逐条切换**（`replay/scheme.rs` 的 `SchemeSwitcher`，读每条的 `scheme` 字段）：整份日志通常只有一套方案，
  所以只在方案串变化时才换码表（`Engine::set_code_table` 收所有权，换一次要克隆 8.9 万条）。日志里是形码但没给 `--wubi` 时，
  那部分只计数——拿拼音的读法喂形码的键会算出看着像真的、实则无意义的命中率。
- `--tune 名=值`（逗号分隔）覆盖个人 n-gram 插值与敲错代价的常数扫网格（名字见 `apps/cli/src/tuning.rs`，Core 侧是 `Engine::set_interpolation` / `set_typo_costs`，壳只用缺省值）。
- `--eval-text <文本>...` 整句评测：把用户自己写的中文文本按标点切句、按词库读音转成全拼，冷启动喂给引擎看整句能不能还原原句
  （首选命中率 / 字准确率 / 查询耗时；不依赖日志里当时选了什么，给整句排序与语言模型的改动当尺子），`--eval-save` 冻结成 `句子\t拼音\t上文` 三列文件，
  之后直接 `--eval-text` 它保证比的是同一份句子（本机的在 `data/eval/sentences.tsv`）。排序、整句、纠错的改动先跑它们再合。

## apps/macos

IMK 输入法，源码按 `app / host / imk / candidates / menubar / preferences` 分目录。

- 输入法菜单（状态项 + 系统输入源菜单）与偏好设置窗口都是配置文件的前端：只写 `config.toml`，`Host::apply_config` 一条通路热加载，激活期间每秒看一次文件 mtime。
  输入方案（`[general] scheme`）也在这里装配：双拼 / 注音设给引擎，形码额外按 `paths::code_table_path()` 挂码表
  （用户目录 `wubi/wubi86.tsv` 优先，包里 `Resources/wubi/` 兜底；找不到只警告并按拼音跑）。
- `apps/macos/scripts/bundle.sh --install` 打包安装到 `~/Library/Input Methods/`（开发用），`--pkg` 做分发用的 pkg（装 `/Library/Input Methods/`，postinstall 跑 `qingjian-macos --register`
  注册、启用并切成当前输入源；签名 / 公证靠 `QINGJIAN_SIGN_IDENTITY` / `QINGJIAN_INSTALLER_IDENTITY` / `QINGJIAN_NOTARY_PROFILE`，没设就 ad-hoc；`QINGJIAN_TARGET` 指定架构，
  成品 `target/pkg/Qingjian-<版本>-<arm64|x86_64>.pkg`）；`scripts/uninstall.sh` 卸载。
- 日志在 `~/Library/Logs/Qingjian/`（按天分文件留 7 天，删了会重建），用户数据与配置在 `~/Library/Application Support/Qingjian/`。
- 配置项：云联想 `[predict]`（偏好设置「云服务」页有「测试连接」按钮：`qingjian_predict::ConnectionTest` 起线程发一条最小请求，`Host` 用独立定时器 `CloudTestMonitor` 轮询结果显示到窗口底部；
  `reasoning_effort` 缺省 `none`，DeepSeek V4 默认思考，不关正文为空）；模糊音 `[fuzzy]` 默认都关；`[general]` 学习语言（`off` 不显示译文）/ 每页候选数 / 翻页键 / 外观 / 竖排横排 / 拼音显示位置 /
  英文模式候选开关 / 中文优先 `chinese_first` / 双拼方案 `shuangpin`（小鹤 / 自然码 / 微软 / 搜狗 / 智能ABC / 小浪 / 首道，空为全拼）/ 日志级别 `log_level`（缺省 info 不含敲的内容，debug 逐键记，热切换）/ 输入日志 `input_log`；
  `[shortcut]` 模式键 v / u、`question_mark`（缺省关，开了空缓冲区敲 `?` 进问字）、上屏第一 / 第二个译词的修饰键 `translation` / `translation_second`、删候选 `delete_candidate`（缺省 shift，用户词整删、词库词清学习）、翻译选中文字 `translate_selection`；
  `[apps] english_candidates_off` 按 bundle identifier 列出英文模式不给候选的应用（缺省终端 / 编辑器 / IDE，`*` 前缀匹配）；
  `[dictionaries] domains` 打开随包的领域词库（`Resources/dicts/` 11 本，缺省只开 `idioms`），`disabled` 关掉用户目录 `dicts/` 里的某本导入词库；
  偏好设置「词库」页随包的可开关、导入的可开关 / 移除，可导入 TSV / Rime yaml / .qj。
- 系统文本替换（系统设置「键盘 → 文本替换」）：`host/config/text_replacements.rs` 从 `NSUserDefaults` 全局域读 `NSUserDictionaryReplacementItems`
  （每条 `{ on, replace, with }`），激活输入法时重读，变了就经 Core `merge_replacements` 并进配置里的自定义短语再 `set_custom_phrases`；
  `[general] system_text_replacements` 开关（缺省开，「自定义短语」页勾选框），内容可能含证件号、地址，日志只记条数。
- 输入法进程由 launchd 拉起，看不到 shell 的环境变量：密钥写进配置同目录的 `.env`（`QINGJIAN_API_KEY=...`，输入法启动时 dotenvy 读入）或 `config.toml` 的 `api_key`。
- 本地整句模型：`bundle.sh` 把 `data/model/`（或 `QINGJIAN_MODEL_DIR`）三件套打进 `Resources/model/`，用户目录 `model/` 优先；`host/model/mod.rs` 在后台线程加载并预热（首次 Metal 编译）后
  `set_async_sentence_scorer` 接上，`refresh` 每键先读应用光标前 64 字给 Engine 当前文、查询后 `schedule_rescoring`，`RescoreMonitor` 停键 80 ms 请求、20 ms 轮询，
  结果到了重查一次只重画当前页（翻过页 / 动过高亮不动）；「云服务」页有开关（`[model] enabled`）。
- 端到端验证可用 `osascript` 的 System Events 往 TextEdit 发按键再读回文本（终端需要辅助功能权限；输入法得在中文模式）。

## apps/windows

一个产品两个 package：`server`（Server 进程：IPC 分派 + Engine + 命名管道 + 自绘候选窗与悬浮状态条）与 `tsf`（TSF 文本服务 DLL，lib 名固定 `qingjian_tsf`），
外加 `settings`（WinUI 3 设置程序）与 `installer`（Inno Setup）。不合成一个 crate，因为 DLL 不能带 Engine 的依赖树，见 `apps/windows/README.md`；
协议类型在 `qingjian-platform::protocol`，设计见 `docs/design/architecture.md`「Windows：TSF」。
输入方案由 `[general] scheme` 一处决定，Server 启动与热加载各装配一次；形码的码表用 `dispatch::code::find_code_table` 找
（用户目录 `wubi/wubi86.tsv` 优先，随包 `assets/wubi/wubi86.tsv` 兜底——走 `assets/` 与 emoji / levels 一致，开发布局也对得上），**路径在启动时定下、热加载不重新找**。
选了形码却没有码表文件时只警告并按拼音跑——配置说五笔、引擎还在拼音是静默错位，宁可吵。

TSF 原有数字 / OEM 标点 / 空格键码按当前布局用 `ToUnicodeEx` 解析（bit 2 避免改变键盘状态），
仅接受单个非代理项 UTF-16 单元。字母、小键盘和 AltGr 处理不变，不保证组合音符输入。
拼音显示位置（`[general] preedit`）在 Windows 上分两处落地：Server 把它读进 `RouterConfig.preedit` 并随 `Frame.preedit_mode`
下发给 DLL，DLL（`com/service/key_sink.rs`）按 `inline()` 决定要不要放行内拼音，Server（`ui/candidates/render_data.rs::window_preedit`）
按 `in_window()` 决定候选窗口顶部画不画拼音行；`window` 模式没有组句范围，光标矩形改从 `com/edit/anchor.rs::caret_rect`（当前选区）量。
**DLL 吃不吃键**由 `key_sink.rs::eats_key(event, composing, translating, english_candidates)` 判：
**英文模式 + 这次不给英文候选**（`[general] english_candidates` 关着，或应用在 `[apps] english_candidates_off` 里；
Server 按当前应用算好、随 `InputSettings.english_candidates` 下发）→ **一个键都不吃**，字母 / 数字 / 标点原样进应用
（打游戏、自绘控件要的原始按键；用户 2026-09-18 提）。中英切换键不受影响：单击判定走 `key_tap` 那条独立路径，
与吃不吃键无关。组句中、翻译评审中照旧吃（先把缓冲区收尾）。
连不上 Server 时 DLL 自己拉起它（`tsf/src/com/service/launch.rs`）：`ShellExecuteW` 起与 DLL 同目录的 `qingjian-server.exe`
（`uiAccess=true` 的 exe 用 `CreateProcess` 报 740），进程内 5 秒冷却 + 跨进程命名互斥体防止砸出一串 Server；
起完清掉重连退避，下一键就试。Server 只在登录时由「启动」文件夹拉起，中途挂了以前只能等下次登录。

候选窗与悬浮状态条默认由 `qingjian-render` 自绘（`ui/painter/` 把 `Frame` / `StatusView` 交出去，`[general] renderer = "system"` 才回落 GDI）。
鼠标命中用的矩形是渲染器给回来的——候选行 `Rendered.rows`、状态条每格 `RenderedStatus.cell_rects`——不在客户区上另算一套：
两套算法一错位就会出现「看得见高亮却点不着」。这些矩形相对**内容区**左上角，而窗口为了四周的阴影比内容区往左上多出一圈，
所以鼠标的客户区坐标得先减掉 `Rendered.content_x/content_y`（顺手按内容区边界判一脚），
漏了这一步命中整体偏右下：竖排看着是「高亮跑到鼠标下面那一行」，只有一行高的整句补全则完全点不中。
悬停底色与可点范围是同一块，格子边缘那圈留白自然点不着。
分层窗口的鼠标命中按像素 alpha 判：**只有 alpha 恰好为 0 的像素才把点击放过去**，而阴影尾部那一大圈 alpha 只有个位数（肉眼几乎看不见）照样吃点击，
表现是「判定区域比看得见的外观大一圈」，状态条下方尤其明显（光从上方来，阴影往下甩得最长）。
所以 `ui/layered/` 在贴上之前把低于 `HIT_ALPHA_FLOOR`（6）的像素整颗抹成全透明，让判定区域贴着外观走。
状态条的「☁」「⚙」也自绘（`crates/qingjian-render/src/{cloud,gear}.rs`）：这两个码位会被 Segoe UI Emoji 接走画成彩色的，不认我们给的颜色。
状态条格子的**宽度基准串**：文字会在几种固定写法之间来回切的格子（模式格 `拼` / `双` / `注` / `拼五` ± 英文的 `英`、Caps 的 `A`，标点格 `，。` / `,.`）用 `StatusCell::text_with_width(text, emphasized, reference)`（GDI 那条是 `CellSpec::width_of`）给一个基准 —— **按基准量宽、按实际文字居中画**，于是同一配置下来回切（中英、全角半角、Caps 亮灭）**整条长度不变**，而改配置（开注释音、开五笔之类）才变一次。渲染器那条在 `renderer/status/mod.rs::status_cell_width` 里取基准，GDI 那条在 `ui/status/mod.rs::render_gdi` 量宽、`paint_cells` 按实际文字居中。**别用「绝对定宽」**：模式格可能出现长文本（旧版会显示「中 · 小浪双拼」），定宽放不下。
模式格的文字与基准都是 `dispatch/status/view.rs::StatusView::mode_text()` / `mode_width()`（纯函数，单测就在那个文件里）：拼音侧一个字（全拼 `拼` / 双拼 `双` / 注音 `注`）+ 形码开着时的 `五`（**两条轴各一个字母，混输就是 `拼五` 这样两个**），英文模式 `英`；**Caps Lock 亮着时只出 `A`**（不与汉字同格，用户 2026-09-18 定的）—— 基准取这份配置里最宽的中文那串。Caps 随 `ClientMessage::ModeChanged` 的 `caps` 字段（**每会话一份**，与 `english` 同层）报来；DLL 在切模式、激活、以及 Caps 键抬起时（`key_sink.rs::note_key_up`）各推一次。
任务栏那个模式图标（语言栏按钮）画哪个字由 Server 挑好下发（`ModeGlyph::for_scheme`：**五笔 > 注音 > 双拼 > 全拼**，两条轴都关时是中）：它是**单张 16px 位图**，一格放不下「拼五」两个字，所以只出优先的那一个；Caps 亮着时 DLL 出「A」、英文模式出「英」，都不看这一项。蒙版在 `apps/windows/tsf/resources/mode/`（`pinyin` / `shuangpin` / `wubi` / `zhuyin` / `zh` / `en` / `caps`，四档 DPI），`Glyph` 变了才 `ModeState::notify()` 让系统重取图标。
**主题色**（`[general] accent`，青简绿 / 经典蓝）由渲染器的 `theme::Accent` 给值，只换三处：**品牌色**（`Palette::accent`，状态条上「中 / 英」「，。」这类强调格与云朵）、**高亮底色**与**悬停底色**；语义色（生词的橙、译文与序号的灰）不跟着变。
深浅各一套（浅底上品牌色要更深才压得住白，深底上要更亮才读得出来），所以是 `Palette::light_with(accent)` / `dark_with(accent)`。
**两条画法都要给**：青简渲染器在 `ui/painter/`（`Painter` 存一份 `Accent`，`configure` 里主题色变了只换配色、**不重建字体库** —— 那是几十毫秒的重扫）；GDI 那条在 `ui/candidates/theme/`，没有 alpha，按各色的不透明度**预混**到背景上写成不透明值。
主题色到达两个窗口的路径：`RenderSettings.accent`（配置类型 `AccentColor`）→ UI 线程 `UiCommand::Configure` → `window.set_accent()` / `status.set_accent()` 各存一份 → 各自 `sync_theme` 里与「造当前主题时用的那份」比对后重建。
**设置页要改 Server 内存里的东西，走「请求文件」而不是直接改数据文件**（2026-09-18，个人词删除）：学习数据在 Server 内存里是权威、**每 60 秒**才落盘一次（`LEARNING_FLUSH_INTERVAL`），设置程序直接改 `user-words.tsv` 会被那份内存数据的下一次落盘**覆盖回去**；而且 `Engine::forget_word` 还会顺手清个人 n-gram 与按输入串的选择，改文件做不到。现在的做法：设置页往 `dirs::forget_requests_path()`（`%APPDATA%\Qingjian\forget-requests.txt`，一行一个词）追加，Server 在 `poll_config_reload`（1 秒一拍）里逐个 `forget`、**立刻 `flush_learning()`**、再把请求文件删掉；幂等（删不掉就下一拍再试）。**判据**：只要「数据在对方内存里 + 对方会周期性落盘」，就别用直接改文件那条路。
整句补全的时机在 `[predict] sentence_trigger`：`idle` 是停键自动请，`tab` 是按下 Tab 现请一次（`Engine::request_sentence_once` 让这一拍破例要句子）；
请出去到结果回来的这段由 `Router.sentence_pending` 驱动候选窗摆「☁ …」。云联想关着时按 Tab 交还应用（缩进 / 跳焦点）。

中英切换键（`[shortcut] switch_mode`）与内置英文模式开关（`[general] english_mode`）得在**按键到达之前**就知道
（单击判定在 `OnTestKeyUp`、是否登记语言栏按钮），但 DLL 跑在每个应用进程里、拿不到 Server 那份配置，
`%APPDATA%\Qingjian` 对 AppContainer 里的商店应用也读不到（那是给输入日志和 `.env` 用的目录，不该加 ACE）。
所以由 **Server 读配置、经协议下发**（`InputSettings`：`OpenSession` 回包带一次，之后每拍 `SyncMode` 跟着走），
DLL 不读文件、不查 mtime。`SessionOpened` 只回过协议版本对得上的 DLL——老的 `open` 是只写不读，
多回一条会被它当成下一次 `Poll` 的应答而报错，那条连接就废了；老 DLL 从 `ModeSync` 那一拍也能拿到同一份（新字段直接忽略）。
线上帧是 **JSON**（`protocol/codec.rs`，4 字节长度前缀 + `serde_json`），所以 `InputSettings` 加字段配 `#[serde(default)]` 就是向后兼容的：老 Server 发的 JSON 里没有 `zhuyin` / `default_mode`，新 DLL 读出来是缺省值而不是报错（`protocol/server.rs` 的 `missing_fields_fall_back_to_defaults` 钉着这条）。

词库导入（设置「词库」页）走 `qingjian-dictionary::import` 转成 `.qj`（空词库拒绝），多选批量、成功的从 `[dictionaries] disabled` 摘掉、页面显示每个文件的结果；
Server 每次轮询比对用户 `dicts\` 的路径 / mtime / 长度快照，配置没变也重载新增、同名更新与移除；配置解析失败时词库沿用上次有效的开关（#36）。

## assets

- `assets/sample/`：手写样例词库与释义表，不是产品数据。
- `assets/wubi/wubi86.tsv`：五笔码表（`dict-convert wubi` 生成，来源与许可见同目录 README，Apache-2.0）。
  `bundle.sh` 拷到 `Resources/wubi/`，Windows 安装器拷到 `{app}\assets\wubi\`（与两边 Server 的找法对齐：macOS `paths::code_table_path`、Windows `dispatch::code::find_code_table`）。
- `assets/icon/windows/`：任务栏模式图标源文件（`mode-{zh,en,caps,zhuyin}.svg`，16×16 单色）。`render-mode-icons.sh` 用 rsvg-convert + magick 栅格化成四档 8 位 alpha 蒙版
  （16/20/24/32px，按字号² 字节，`include_bytes!` 进 DLL），`render-mode-icons.ps1` 是 Windows 上那条等价路径（没有 rsvg / magick 时用）。中 / 英 / A 三张是设计稿手画的轮廓；
  **「注」没有设计稿**，由 .ps1 取 `GraphicsPath.AddString` 的字形轮廓（缺省 Noto Sans SC Black，笔画重量与设计稿最接近）归一到 16×16（ink 高度 13.5，设计稿「中」14、「英」13）、同时写 `mode-zhuyin.svg` 与四档蒙版。
  **Windows PowerShell 5.1 按 ANSI 解码没有 BOM 的 .ps1**：文件里的中文注释会被解成乱码，尾字节吃掉换行把下一行代码并进注释（踩过：`$prev = New-Object ...` 被吞，报「FromImage 参数为 null」）—— .ps1 必须带 UTF-8 BOM。
- `assets/emoji/emoji-zh.tsv` / `emoji-en.tsv`：Unicode CLDR 中文 / 英文 annotations 转出的 emoji 表（Unicode License v3，可发布；中文词与英文词各配 emoji，两张表加载时合成一张），
  `cargo run --release -p qingjian-dict-convert -- --out-dir assets/emoji emoji --language zh data/cldr/annotations-zh.json data/cldr/annotationsDerived-zh.json`（en 同理）。
- 英文词表词频：`uv run tools/corpus/english_frequency.py data/generated/english.tsv -o data/generated/english-frequency.tsv`，再 `... english <词表> --frequency <那个文件>`。

## tools/gloss-gen

用 LLM 批量生成释义表：`cargo run --release -p qingjian-gloss-gen -- generate`（密钥读 `QINGJIAN_API_KEY`，结果 JSONL 在 `data/generated/`，不进 git、可续跑，`--limit 80` 试跑）
再 `... export`（写 `glossary-{en,ja}.tsv`，产品数据在 `assets/glossary/`，见那里的 README；格式 `词\t词性. 译词[|假名]`）。CLI 与 bundle.sh 用的就是这两个文件。

## tools/dict-convert

产品数据的生成工具，输出到 `data/generated/`（gitignore）。

- `lexicon`：从 `assets/lexicon/`（自建词库源：规范字 + 常用词 + THUOCL 领域词）加 Unihan 读音（`data/unihan/Unihan_Readings.txt`）、LLM 多音字标注（`gloss-gen pinyin`，
  结果 `data/generated/pinyin-llm.jsonl`，不进 git）、语料词频（`lm-unigram.tsv`）建基础词库 `dict.tsv`（8.7 万条），并把 THUOCL 领域词按语料次数 < 50 拆成
  `dicts/<领域>.tsv` + `.qj`（11 本、13 万条，`--domain-keep-min`），流程见 `assets/lexicon/QINGJIAN.md`；`--extra-words` 并入人工挑的领域词 `assets/lexicon/domain_words.tsv`。
- `english`：转 `assets/lexicon/05_english/00_all_words.tsv`；`cedict`：释义表备用来源。
- `wubi`：Rime 形码码表（`.dict.yaml`，极点 86 五笔）→ `词\t编码\t词频`（`wubi.rs`，`--name` 决定文件名，缺省 `wubi86.tsv`）。
  **码表自带的权重不用**——那是码表顺序不是语料词频，用了同一个词在形码下和在拼音下会排得不一样；词频从青简词库按**词面**交叉回填，
  词库里没有的词给 `UNKNOWN_FREQUENCY = 1`。解析复用 `qingjian_dictionary::import::rime`（与用户导入 Rime 词库同一个解析器，
  形码码表的第二列是编码，格式一样）。词库还没收的词不收（码表整张进，不做取码推导，见 `docs/plan/wubi.md`）。
- `bigram`：统计语料；`--phrases` 给短语层、`--brand` 给品牌词（`assets/lexicon/brand.tsv`，青简 210）与中英混杂词（`mixed_words.tsv`，C盘 / B站：合成计数要成分词在语料里，C 不是 token，只能直接给一元，次数对着同音竞争词定），领域词也走合成计数（语料里只有几十次的词当 token 统计会吸走成分词的二元证据）。
- `mine`：从语料挖词库没收的高频词并过滤（`oov_filter.rs`：虚词规则 + 相邻字对 PMI≥3，`--candidates` 只重过滤）。
- `phrases`：挖短语层（两遍扫语料：相邻两词、两段二元都够频的相邻三词，总次数与对话语料次数都 ≥ 2000 + 边界规则，读音由成分词拼出；我的 / 不知道 / 有没有 这类常用词表不收的组合，
  `assets/lexicon/phrases.tsv`；词库已并入过短语时重跑加 `--refresh`）。
- `pack dict|lm|glossary`：打 `.qj`（释义表也进容器）。
