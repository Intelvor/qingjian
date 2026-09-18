//! 候选生成：按模式分派查询，整句转换与词级查找，位置展开。

use super::*;

mod code;
mod english_tail;
mod result;
mod shifted;
mod snapshot;

pub(crate) use english_tail::EnglishTail;
pub use result::Query;
pub(super) use result::join_marked;
pub(super) use snapshot::QuerySnapshot;

impl Engine {
    /// 解析当前缓冲区并生成排好序的候选。**不带译文**，译文由 [`Self::annotate`] 补。
    ///
    /// 光标停在拼音中间时只按光标前的那段算候选（`ni|hao` 出 你），光标后的拼音留着，
    /// 上屏之后接着组句；见 [`Composition::scope`]。
    pub fn query(&self) -> Result<Query, ParseError> {
        self.last_rescored.set(false);
        let mut query = match self.query_inner() {
            Ok(query) => query,
            Err(error) => {
                if !self
                    .custom_phrases
                    .iter()
                    .any(|p| p.enabled && p.code == self.composition.scope())
                {
                    return Err(error);
                }
                Query::custom_only(
                    self.composition.text(),
                    self.composition.cursor(),
                    self.shuangpin.is_some() || self.zhuyin,
                    self.composition.scope(),
                    self.marked_rest(self.composition.rest()),
                )
            }
        };
        self.insert_custom_phrases(&mut query.candidates.items);
        // 给输入日志留个摘要：上屏时才知道选了什么，这里才知道看到了什么
        let pinyin = match &query.correction {
            Some(correction) => correction.segmentation.joined("'"),
            None => join_marked(&query.segmentations, &query.tail),
        };
        *self.last_query.borrow_mut() = Some(QuerySnapshot {
            scope: self.composition.scope().to_owned(),
            pinyin,
            corrected: query.correction.is_some(),
            candidates: query
                .candidates
                .items
                .iter()
                .take(QuerySnapshot::MAX_CANDIDATES)
                .map(|c| c.text.clone())
                .collect(),
            rescored: self.last_rescored.get(),
        });

        if self.traditional
            && let Some(opencc) = &self.opencc
        {
            for candidate in &mut query.candidates.items {
                if matches!(
                    candidate.kind,
                    CandidateKind::Chinese | CandidateKind::Sentence | CandidateKind::Cloud
                ) {
                    let traditional_text = opencc.convert(&candidate.text);
                    self.traditional_map
                        .borrow_mut()
                        .insert(traditional_text.clone(), candidate.text.clone());
                    candidate.text = traditional_text;
                }
            }
        }

        Ok(query)
    }

    pub(super) fn query_inner(&self) -> Result<Query, ParseError> {
        let start = Instant::now();
        let keys = self.composition.scope();
        let rest = self.marked_rest(self.composition.rest());
        if self.english_mode {
            return Ok(self.query_english(keys, rest, start));
        }
        if self.modes().is_expression(keys, self.zhuyin) {
            return Ok(self.query_expression(keys, rest, start));
        }
        if self.modes().is_question(keys, self.zhuyin) {
            return Ok(self.query_question(keys, rest, start));
        }
        if is_raw(keys, self.modes(), self.shuangpin, self.zhuyin) {
            return Ok(self.query_raw(keys, rest, start));
        }
        // 形码与拼音是两条平行的管线，在进切分之前分岔。放在这里是为了让 `?` 问字与
        // `-` 直输段仍然先分派出去：形码下 `v` / `u` / `i` 是字根键，模式键已由 `modes()` 让位。
        match self.code.is_some() {
            // 只用形码：拼音侧整个不走（`[general] scheme = "none"`）
            true if !self.phonetic => Ok(self.query_code(keys, rest, start)),
            // 混输：两边都出候选
            true => self.query_mixed(keys, rest, start),
            false => self.query_phonetic(keys, rest, start),
        }
    }

    /// 拼音侧（全拼 / 双拼 / 注音）的候选生成：整段作用域是一串读音。
    fn query_phonetic(
        &self,
        keys: &str,
        rest: String,
        start: Instant,
    ) -> Result<Query, ParseError> {
        // 双拼先解成全拼（音节间已用 `'` 连好，切分没有歧义），之后与全拼同路；解不动的键当尾巴
        // Shift 大写进组句：拼音仍按**整段小写**走普通查词/整句（中途 `AI` 后面不能断），
        // 候选生成后再合并英文与「开头大写+剩余拼音」的大写字面（见 `merge_shifted_extras`）。
        let decoded = self.decode(keys);
        let scope: &str = decoded.as_ref().map_or(keys, |d| d.pinyin());
        // 末尾是英文词（`woxiangxuehaorust`）：拼音候选与整句只按头段算，尾段整个跟在整句后面。
        // 整段也能读成拼音时（`database`、`…rust` 当简拼）两种读法比分，英文赢了才按头段算，
        // 输了整段按拼音读、英文读法排在拼音整句后面
        let english_tail = if decoded.is_none() {
            self.split_english_tail(keys)
        } else {
            None
        };
        let head_wins = english_tail
            .as_ref()
            .is_some_and(|t| !t.competes || self.mixed_beats_plain(keys, t));
        let parsed = match (&decoded, &english_tail) {
            (Some(d), _) => d
                .segmentation()
                .map(|s| (vec![s], d.tail()))
                .ok_or(ParseError::NoSegmentation),
            (None, Some(tail)) if head_wins => {
                parser::segment(&keys[..tail.head_len]).map(|s| (s, ""))
            }
            _ => segment_longest_prefix(keys),
        };
        // 连第一个字母都切不动（`impor`）：拼音这边没戏，但英文词 / 补全、快捷候选还可以有
        let (segmentations, tail) = match parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                let mut items = Vec::new();
                self.insert_english(&mut items, true);
                self.insert_shortcuts(&mut items, keys);
                if items.is_empty() {
                    return Err(error);
                }
                return Ok(Query {
                    segmentations: Vec::new(),
                    candidates: CandidateList { items },
                    tail: keys.to_owned(),
                    text: self.composition.text().to_owned(),
                    cursor: self.composition.cursor(),
                    rest,
                    decoded_keys: self.shuangpin.is_some() || self.zhuyin,
                    typed_display: decoded.as_ref().map(|d| d.marked()),
                    correction: None,
                    timings: Timings {
                        parse: start.elapsed(),
                        lookup: Duration::ZERO,
                        rank: Duration::ZERO,
                    },
                });
            }
        };
        // 拼音「不像话」时试拼写纠错；纠正生效则按纠正后的切分查词，原串只用来记学习与显示
        let unlikely = correction::unlikely_pinyin(segmentations.first(), tail)
            || correction::trailing_single_letter(segmentations.first());
        let correction = if unlikely {
            self.active_correction(scope)
        } else {
            None
        };
        let (segmentations, tail): (Vec<Segmentation>, &str) = match &correction {
            Some(c) => (vec![c.segmentation.clone()], ""),
            None => (segmentations, tail),
        };
        let parse = start.elapsed();

        let start = Instant::now();
        let mut scored = Vec::new();
        // 不同切分共享很多前缀（`zh g d o…` 的各种切法前几段一样），同一次查询里同一个模式只查一遍
        let mut memo: HashMap<String, Vec<Match<'_>>> = HashMap::new();
        for (seg_index, segmentation) in segmentations.iter().enumerate() {
            let patterns = segmentation.patterns();
            let count = patterns.len();
            if count == 0 {
                continue;
            }
            let last = &segmentation.syllables[count - 1];
            // 模糊音：按切分原样扩；非末尾残缺/简拼的切分不套（`zh'en` 叠起来会切进生僻词）。
            // 末尾是完整音节但还能往下敲时（`ha`/`zhen`）：
            // - 敲的原文 + 声母类模糊写法按**前缀**查（f/h 下 `kaiha` 要能出 开放）
            // - 模糊前缀**不收**「更长的单音节词」（`zhen` 开 z/zh 时 `zen` 前缀不该吃进 增/zeng）
            let inner_incomplete = patterns
                .iter()
                .take(count.saturating_sub(1))
                .any(|p| !p.complete);
            let expanded = if inner_incomplete {
                Expanded::exact(&patterns)
            } else {
                self.fuzzy.expand(&patterns)
            };
            let positions = expanded.positions();
            let abbreviated = abbreviated_count(&patterns);
            let mut hits = self.lookup_all(&positions);
            if !inner_incomplete
                && last.complete
                && decoded.is_none()
                && parser::is_syllable_prefix(&last.text)
            {
                let typed_last = patterns[count - 1].text;
                // 前缀写法：敲的原文 + 同长度同韵母的换声母写法（`ha`→`fa`；`zhen`→`zen` 长度不同，不加）
                let mut prefix_forms = vec![typed_last.to_owned()];
                for form in &positions[count - 1] {
                    let t = typed_last.as_bytes();
                    let f = form.text.as_bytes();
                    if form.text != typed_last
                        && t.len() == f.len()
                        && t.len() >= 2
                        && t[0] != f[0]
                        && t[1..] == f[1..]
                    {
                        prefix_forms.push(form.text.to_owned());
                    }
                }
                for (form_index, form) in prefix_forms.iter().enumerate() {
                    let mut prefix_patterns = segmentation.patterns();
                    prefix_patterns[count - 1].text = form.as_str();
                    prefix_patterns[count - 1].complete = false;
                    let prefix_hits =
                        self.lookup_all(&Expanded::exact(&prefix_patterns).positions());
                    for hit in prefix_hits {
                        let single = hit.syllables().count() == 1;
                        let exact_form = hit.syllables().next() == Some(form.as_str());
                        // 模糊前缀不收更长的单音节词（避免 zen 前缀吃进 zeng）
                        if form_index > 0 && single && !exact_form {
                            continue;
                        }
                        if !hits.iter().any(|h| h.text == hit.text) {
                            hits.push(hit);
                        }
                    }
                }
            }
            scored.reserve(hits.len());
            for hit in hits {
                let full_last = last.complete
                    && hit.syllables().nth(count - 1) == Some(patterns[count - 1].text);
                scored.push(Scored {
                    hit,
                    full_last,
                    coverage: segmentation.letters(),
                    abbreviated,
                    weight: self.learner.weight(hit.text),
                    penalty: expanded.penalty(hit.syllables()),
                });
            }
            // 输入的前缀也出候选（`kaifazhe` → 开发、开），否则长句没法逐词上屏。
            // 只收音节数正好等于前缀长度的词，更长的词会与输入后面的音节冲突。
            // **只在最优切分上做**：备选切分里的简拼前缀（`zhen` 的 `z'hen` → 拿 `z` 查）
            // 会把所有 z* 单音节词塞进候选，那是不合适的地方切出来的生僻读法。
            if seg_index == 0 {
                for prefix_len in (1..count).rev() {
                    let prefix = &patterns[..prefix_len];
                    let prefix_letters: usize = prefix.iter().map(|p| p.text.len()).sum();
                    let hits = memo
                        .entry(pattern_key(prefix))
                        .or_insert_with(|| self.lookup_exact_all(&positions[..prefix_len]));
                    let abbreviated = abbreviated_count(prefix);
                    for hit in hits.iter().copied() {
                        scored.push(Scored {
                            // 对整个输入来说它不是精确命中，只是覆盖了前面一部分
                            hit: Match {
                                exact: false,
                                ..hit
                            },
                            full_last: true,
                            coverage: prefix_letters,
                            abbreviated,
                            weight: self.learner.weight(hit.text),
                            penalty: expanded.penalty(hit.syllables()),
                        });
                    }
                }
            }
        }
        let lookup = start.elapsed();

        let start = Instant::now();
        // 再往后翻也翻不到的候选不必再造：单字母简拼能命中两万个词，排完序只留前面这些。
        // 同输入串（候选覆盖的那段字母）下选过的优先；上下文是上一个上屏的词（句首为 None）：
        // `ba` 在「做了」后面出 吧、句首出 把
        let log_total = (self.total_frequency() as f64).max(1.0).ln();
        let letters = choice_key(scope, scope.len());
        let input_letters = scope.chars().filter(|c| *c != '\'').count();
        ranking::rank(&mut scored, MAX_CANDIDATES, |item| {
            let hit = &item.hit;
            // 纠错生效时覆盖的是纠正后的字母，换算回原串再查「这个输入串下选过什么」
            let covered = correction
                .as_ref()
                .map_or(item.coverage, |c| c.edit.to_original(item.coverage));
            let choice = letters
                .get(..covered)
                .map_or(0, |input| self.learner.choice_weight(input, hit.text));
            let log_prob = sentence::transition_log_prob(
                &*self.language_model,
                self.personal(),
                self.chain.context(),
                hit.text,
                sentence::fallback_log_prob(hit.frequency, log_total),
            );
            // 自身拼音比用户敲的还长：略降权（前缀长词、整句型词条）
            let hit_letters: usize = hit.syllables().map(|s| s.len()).sum();
            let log_prob = log_prob - ranking::excess_pinyin_penalty(hit_letters, input_letters);
            (choice, log_prob)
        });
        let mut items: Vec<Candidate> = scored
            .into_iter()
            .map(|s| Candidate {
                text: s.hit.text.to_owned(),
                kind: CandidateKind::Chinese,
                syllables: s.hit.syllables().map(str::to_owned).collect(),
                reading: None,
                translation: None,
            })
            .collect();
        // 中文优先：整句先进去占第一，英文词紧跟其后（第二）；关掉时英文词先进、整句排在开头的英文后面
        if self.chinese_first {
            self.insert_sentence(
                &mut items,
                &segmentations,
                correction.is_none(),
                english_tail.as_ref().filter(|_| correction.is_none()),
                head_wins,
            );
            self.insert_english(&mut items, unlikely);
        } else {
            self.insert_english(&mut items, unlikely);
            self.insert_sentence(
                &mut items,
                &segmentations,
                correction.is_none(),
                english_tail.as_ref().filter(|_| correction.is_none()),
                head_wins,
            );
        }
        // 快捷候选按敲的键认（`rq` 日期），双拼下也是
        self.insert_shortcuts(&mut items, keys);
        self.insert_emoji(&mut items);
        if decoded.is_none() && self.composition.has_shifted() {
            self.merge_shifted_extras(&mut items, &self.composition.typed_scope());
        }
        let rank = start.elapsed();

        // 按头段算时英文尾段不参与拼音候选，显示上跟在切分后面：`wo'xiang'xue'hao'rust`
        let tail = english_tail
            .as_ref()
            .filter(|_| head_wins)
            .map_or(tail, |t| &keys[t.head_len..]);
        let typed_display = decoded.as_ref().map(|d| d.marked()).or_else(|| {
            // Shift 大写：preedit 按敲的原样显示，避免 join_marked 在中途大写上拼出乱切分
            self.composition
                .has_shifted()
                .then(|| self.composition.typed_scope())
        });
        Ok(Query {
            segmentations,
            candidates: CandidateList { items },
            tail: tail.to_owned(),
            text: self.composition.text().to_owned(),
            cursor: self.composition.cursor(),
            rest,
            decoded_keys: self.shuangpin.is_some() || self.zhuyin,
            typed_display,
            correction,
            timings: Timings {
                parse,
                lookup,
                rank,
            },
        })
    }

    /// 表达式模式（`v` 开头）：不解析拼音，候选是算式结果 / 中文数字，再加上整段是英文词的情况（`very`）。
    /// preedit 原样显示输入。
    pub(super) fn query_expression(&self, scope: &str, rest: String, start: Instant) -> Query {
        let mut items = shortcut::candidates(scope, self.modes().expression, &jiff::Zoned::now());
        if let Some(word) = self.english.as_ref().and_then(|english| english.get(scope)) {
            items.push(Candidate {
                text: word.to_owned(),
                kind: CandidateKind::English,
                syllables: Vec::new(),
                reading: None,
                translation: None,
            });
        }
        Query {
            segmentations: Vec::new(),
            candidates: CandidateList { items },
            tail: scope.to_owned(),
            text: self.composition.text().to_owned(),
            cursor: self.composition.cursor(),
            rest,
            decoded_keys: self.shuangpin.is_some() || self.zhuyin,
            typed_display: None,
            correction: None,
            timings: Timings {
                parse: Duration::ZERO,
                lookup: Duration::ZERO,
                rank: start.elapsed(),
            },
        }
    }

    /// 英文直输段：唯一候选就是原文（`no-way`），空格 / 回车都上屏它；preedit 原样显示。
    pub(super) fn query_raw(&self, scope: &str, rest: String, start: Instant) -> Query {
        let items = vec![Candidate {
            text: scope.to_owned(),
            kind: CandidateKind::English,
            syllables: Vec::new(),
            reading: None,
            translation: None,
        }];
        Query {
            segmentations: Vec::new(),
            candidates: CandidateList { items },
            tail: scope.to_owned(),
            text: self.composition.text().to_owned(),
            cursor: self.composition.cursor(),
            rest,
            decoded_keys: self.shuangpin.is_some() || self.zhuyin,
            typed_display: None,
            correction: None,
            timings: Timings {
                parse: Duration::ZERO,
                lookup: Duration::ZERO,
                rank: start.elapsed(),
            },
        }
    }

    /// 英文模式：敲的字母原样显示，候选是英文词表的精确词、前缀补全与拼错纠正（见 [`english::suggest`]），
    /// 词表没装就没有候选。emoji 照配，但排在所有词后面：选词靠上下键，emoji 夹在词中间会挡路。
    pub(super) fn query_english(&self, scope: &str, rest: String, start: Instant) -> Query {
        let mut items: Vec<Candidate> = english::suggest(
            &self.english_lists(),
            scope,
            |text| self.learner.weight(text),
            ENGLISH_MODE_CANDIDATES,
        )
        .into_iter()
        .map(|text| Candidate {
            text,
            kind: CandidateKind::English,
            syllables: Vec::new(),
            reading: None,
            translation: None,
        })
        .collect();
        self.insert_emoji(&mut items);
        items.sort_by_key(|c| c.kind == CandidateKind::Emoji);
        Query {
            segmentations: Vec::new(),
            candidates: CandidateList { items },
            tail: scope.to_owned(),
            text: self.composition.text().to_owned(),
            cursor: self.composition.cursor(),
            rest,
            decoded_keys: self.shuangpin.is_some() || self.zhuyin,
            typed_display: None,
            correction: None,
            timings: Timings {
                parse: Duration::ZERO,
                lookup: Duration::ZERO,
                rank: start.elapsed(),
            },
        }
    }

    /// 问字模式（问字键或 `?` 开头）：拼音问题本地没有候选，preedit 显示前缀加切分好的问题拼音，答案等云端；
    /// 十六进制码点（`u4e00`、`u+1f600`）本地直接给出那个字符。
    pub(super) fn query_question(&self, scope: &str, rest: String, start: Instant) -> Query {
        let body = self.modes().question_body(scope, self.zhuyin);
        let prefix = &scope[..scope.len() - body.len()];
        let (candidates, tail) = match shortcut::unicode_form(body) {
            Some(text) => (
                CandidateList {
                    items: vec![Candidate {
                        text,
                        kind: CandidateKind::Shortcut,
                        syllables: Vec::new(),
                        reading: None,
                        translation: None,
                    }],
                },
                scope.to_owned(),
            ),
            None => (
                CandidateList::default(),
                format!("{prefix}{}", self.marked_rest(body)),
            ),
        };
        Query {
            segmentations: Vec::new(),
            candidates,
            tail,
            text: self.composition.text().to_owned(),
            cursor: self.composition.cursor(),
            rest,
            decoded_keys: self.shuangpin.is_some() || self.zhuyin,
            typed_display: None,
            correction: None,
            timings: Timings {
                parse: start.elapsed(),
                lookup: Duration::ZERO,
                rank: Duration::ZERO,
            },
        }
    }

    /// 整句插在候选列表的哪个位置。
    ///
    /// 设计哲学：整句模型仍要出结果，不浪费；但不无条件压过词级。
    /// - 词级首选是双字词、且整句**以它开头**（「逐步」→「逐步分析问题」）→ 整句仍在英文候选之后的最前；
    /// - 整句**不以**该词开头（「内部」vs「那不是威廉」、「逐步」vs「住不分析问题」）→ 整句跟在它后面。
    fn sentence_insert_position(plain: &Candidate, items: &[Candidate]) -> usize {
        let english = leading_english(items);
        if plain.kind != CandidateKind::Sentence {
            return english;
        }
        let Some(index) = items.iter().position(|c| {
            c.kind == CandidateKind::Chinese
                && c.syllables.len() >= 2
                && c.text.chars().count() >= 2
        }) else {
            return english;
        };
        let first = &items[index];
        // 同一段读音的两种展示（词与整句音节数相同），或整句以该词开头 → 整句仍可排前面；
        // 词只是整句的前缀且文本也接不上（「内部」vs「那不是威廉」）→ 整句跟在词后面。
        if first.syllables.len() == plain.syllables.len() || plain.text.starts_with(&first.text) {
            english
        } else {
            (index + 1).min(items.len())
        }
    }

    /// 整句候选。没有英文尾段时是整段拼音的转换（[`Self::plain_sentence`]），排在开头的英文候选之后。
    /// 有英文尾段且英文读法胜出（`head_wins`）时，头段的转换加上那个词排第一（`woxiangxuehaorust` → 我想学好rust），
    /// 整段也能读成拼音的再把拼音读法的整句放在第二；英文读法输了就不出（`diaoyong` 不出 掉Yong），
    /// 免得把真正要的候选往后挤。
    /// `typos` 为假时词图里不加敲错边（整段一处编辑的纠错已经生效，不在纠正后的拼音上再猜第二处）。
    pub(super) fn insert_sentence(
        &self,
        items: &mut Vec<Candidate>,
        segmentations: &[Segmentation],
        typos: bool,
        english_tail: Option<&EnglishTail>,
        head_wins: bool,
    ) {
        let Some(best) = segmentations.first() else {
            return;
        };
        let keys = self.composition.scope();
        let first_segmentation = |text: &str| parser::segment(text).ok()?.into_iter().next();
        match english_tail {
            Some(tail) if head_wins => {
                if let Some(mixed) = self.mixed_sentence(best, tail, typos) {
                    items.insert(0, mixed);
                }
                if tail.competes
                    && let Some(full) = first_segmentation(keys)
                    && let Some((plain, _)) =
                        self.plain_sentence(items, std::slice::from_ref(&full), typos)
                {
                    let position = items.len().min(1);
                    items.insert(position, plain);
                }
            }
            _ => {
                if let Some((plain, model_chose_alt)) =
                    self.plain_sentence(items, segmentations, typos)
                {
                    let input_letters = keys.chars().filter(|c| *c != '\'').count();
                    let sent_letters: usize = plain.syllables.iter().map(|s| s.len()).sum();
                    // 拼音长度超出输入过多：整句排后面，避免压过词级候选
                    let longer_than_input = sent_letters > input_letters * 2;
                    let position = if model_chose_alt && !longer_than_input {
                        leading_english(items)
                    } else {
                        Self::sentence_insert_position(&plain, items)
                    };
                    items.insert(position, plain);
                }
            }
        }
    }

    /// 最优切分之外再试几条整句。太靠后的切分离最长切分太远（多是拆碎音节的读法），试了只是白花时间。
    const ALTERNATIVE_SEGMENTATIONS: usize = 4;

    /// 整段拼音的整句候选：返回 `(候选, 模型是否选了非 best 的切分)`。
    ///
    /// **同音节数的切分各转一次整句，语言模型分高者胜**（2026-09-19）：切分排序偏爱「前面音节更长」
    /// （`heng'an` 压过 `hen'gan`），但更长的前段不一定是用户要说的那句话（`henganrende` 应出
    /// 「很感人的」而不是地名「恒安人的」）。模型要比过才知道，不能只信 best。
    /// 非末尾有简拼的切分、音节数与 best 不一致的转换仍不参与。
    pub(super) fn plain_sentence(
        &self,
        items: &mut Vec<Candidate>,
        segmentations: &[Segmentation],
        typos: bool,
    ) -> Option<(Candidate, bool)> {
        let best = segmentations.first()?;
        let best_len = best.syllables.len();
        if best_len < 2 {
            return None;
        }
        let mut winner: Option<(usize, Conversion)> = None;
        for (index, segmentation) in segmentations
            .iter()
            .take(Self::ALTERNATIVE_SEGMENTATIONS + 1)
            .enumerate()
        {
            if segmentation.syllables.len() != best_len {
                continue;
            }
            // 简拼切分也要参与整句：`fchhle` 这类输入末尾的 `le`/`l` 就是「了」，
            // 整句模型仍要能吐出「非常好了」；过碎的读法靠音节数对齐与插入位置降权兜住。
            let Some(conversion) = self.convert_sentence(&segmentation.patterns(), typos) else {
                continue;
            };
            if conversion.has_placeholder() || conversion.syllables.len() != best_len {
                continue;
            }
            winner = Some(match winner.take() {
                None => (index, conversion),
                Some((prev_i, prev)) => {
                    let pick_new = match prev.score.partial_cmp(&conversion.score) {
                        Some(std::cmp::Ordering::Less) => true,
                        Some(std::cmp::Ordering::Greater) => false,
                        _ => prev.altered() && !conversion.altered(),
                    };
                    if pick_new {
                        (index, conversion)
                    } else {
                        (prev_i, prev)
                    }
                }
            });
        }
        let (win_index, mut conversion) = winner?;
        let model_chose_alt = win_index > 0;
        // 词级已有同输入的完整词时，敲错边整句退回原样读音
        if conversion.altered() {
            let letters = best.joined("");
            let spelled_exactly = items
                .iter()
                .any(|c| c.kind == CandidateKind::Chinese && c.syllables.concat() == letters);
            if spelled_exactly
                && let Some(clean) = self.convert_sentence(&best.patterns(), false)
                && !clean.has_placeholder()
                && clean.syllables.len() == best_len
            {
                conversion = clean;
            }
        }
        if conversion.has_placeholder() || conversion.syllables.len() != best_len {
            return None;
        }
        // 整段本来就是一个词时不出整句；敲错边读成的一个词（`meiganxi` → 没关系）作普通中文候选
        let kind = if conversion.word_count() >= 2 {
            CandidateKind::Sentence
        } else if conversion.altered() {
            CandidateKind::Chinese
        } else {
            return None;
        };
        if let Some(index) = items.iter().position(|c| c.text == conversion.text) {
            if items[index].syllables == conversion.syllables {
                return None;
            }
            items.remove(index);
        }
        Some((
            Candidate {
                text: conversion.text,
                kind,
                syllables: conversion.syllables,
                reading: None,
                translation: None,
            },
            model_chose_alt,
        ))
    }

    /// 跑一次整句转换：主词库 + 用户词（含模糊音与敲错写法，命中的按代价扣分），静态语言模型与个人 n-gram 插值，用户选择次数加分。
    /// `typos` 为假时不加敲错边。
    pub(super) fn convert_sentence(
        &self,
        patterns: &[qingjian_dictionary::SyllablePattern<'_>],
        typos: bool,
    ) -> Option<Conversion> {
        self.convert_sentence_with(patterns, typos, false)
    }

    /// 同 [`Self::convert_sentence`]，`whole` 为真时末尾单字母也读（[`sentence::convert_whole`]），只给比分用。
    /// 接了神经重打分器时取前 [`RESCORE_PATHS`] 条路径，按「路径分 + λ·(神经分 − 静态分)」重排（[`Self::rescore_paths`]）：
    /// 神经分替换的是静态二元模型那部分判断，个人 n-gram 插值、用户加分、敲错代价原样保留，尺度也不变（纠错代价等常数照旧适用）。
    /// 返回重排后的第一条（`score` 换成重排后的分，好与别的读法比）；只有一条路径或模型还没给分时原样返回。
    pub(super) fn convert_sentence_with(
        &self,
        patterns: &[qingjian_dictionary::SyllablePattern<'_>],
        typos: bool,
        whole: bool,
    ) -> Option<Conversion> {
        let dictionaries = self.all_dictionaries();
        let expanded = self.expand_positions(patterns, typos);
        // 末尾单字母简拼 + 前面带 `'`（`wo'...'ni'd` → 的）：用户显式把它当独立音节，不是上一字没打完的前缀
        let forced_tail = patterns.last().is_some_and(|p| {
            !p.complete && self.composition.scope().ends_with(&format!("'{}", p.text))
        });
        let k = if self.has_sentence_scorer() {
            RESCORE_PATHS
        } else {
            1
        };
        let mut paths = sentence::convert_paths(
            &dictionaries,
            &expanded.positions(),
            whole,
            forced_tail,
            k,
            &*self.language_model,
            self.personal(),
            |text| self.learner.weight(text),
            |index, syllable| expanded.cost(index, syllable),
            &mut self.span_cache.borrow_mut(),
        );
        // 与最优路径差得太远的不参与：那种差距多半是个人 n-gram 拉开的
        if paths.len() > 1 {
            let floor = paths[0].score - self.neural_margin;
            paths.retain(|p| p.score >= floor);
            self.rescore_paths(&mut paths);
        }
        paths.into_iter().next()
    }

    /// 每个位置的写法：敲的原样、模糊音，再加音节级敲错变体（`correction::typo`）当带代价的边，
    /// 代价按类别定、按个人敲错表打折。太短的输入（不到 [`correction::MIN_LETTERS`]）、双拼、非末尾带简拼的切分不加敲错变体：
    /// 短串一处编辑几乎总能凑出别的词，双拼敲错一键换掉的是整个声母 / 韵母。不完整的位置（简拼、前缀）本来就按前缀查，不加。
    pub(super) fn expand_positions(
        &self,
        patterns: &[qingjian_dictionary::SyllablePattern<'_>],
        typos: bool,
    ) -> Expanded {
        let mut expanded = self.fuzzy.expand(patterns);
        if !typos {
            return expanded;
        }
        let letters: usize = patterns.iter().map(|p| p.text.len()).sum();
        // 非末尾有简拼 / 残缺音节的切分（`kai f a`）本来就不是用户敲的原话，不在它上面再猜敲错
        let inner_abbreviated = patterns
            .iter()
            .take(patterns.len().saturating_sub(1))
            .any(|p| !p.complete);
        if self.shuangpin.is_some() || letters < correction::MIN_LETTERS || inner_abbreviated {
            return expanded;
        }
        for (index, pattern) in patterns.iter().enumerate() {
            if !pattern.complete {
                continue;
            }
            for (text, kind) in typo::variants(pattern.text) {
                let accepted = self.learner.typo_count(pattern.text, text);
                expanded.push_alternative(index, text, self.typo_costs.typo_cost(*kind, accepted));
            }
        }
        expanded
    }

    /// 本地整句转换把最优切分转成的汉字，给云端当参考（问字模式里就是问题的汉字形式）；转不出或有占位音节为空。
    pub(super) fn local_guess(&self, segmentations: &[Segmentation]) -> String {
        segmentations
            .first()
            .and_then(|best| self.convert_sentence(&best.patterns(), true))
            .filter(|conversion| !conversion.has_placeholder())
            .map(|conversion| conversion.text)
            .unwrap_or_default()
    }

    /// 主词库与用户词一起查（每个位置多种写法）。用户词是用户自己选过的（云联想接受的词等），排序上靠 weight 自然靠前。
    pub(super) fn lookup_all(
        &self,
        positions: &[Vec<qingjian_dictionary::SyllablePattern<'_>>],
    ) -> Vec<Match<'_>> {
        let mut hits = self.dictionary.lookup_pattern_alt(positions);
        for dictionary in self.all_dictionaries().into_iter().skip(1) {
            hits.extend(dictionary.lookup_pattern_alt(positions));
        }
        hits
    }

    /// 只要音节数正好等于位置数的词，主词库与用户词一起查。
    pub(super) fn lookup_exact_all(
        &self,
        positions: &[Vec<qingjian_dictionary::SyllablePattern<'_>>],
    ) -> Vec<Match<'_>> {
        let mut hits = self.dictionary.lookup_exact_alt(positions);
        for dictionary in self.all_dictionaries().into_iter().skip(1) {
            hits.extend(dictionary.lookup_exact_alt(positions));
        }
        hits
    }
}

/// 排在开头的英文候选有几条（整段是英文词、不像拼音带出的英文补全）：整句插在它们后面。
fn leading_english(items: &[Candidate]) -> usize {
    items
        .iter()
        .take_while(|c| c.kind == CandidateKind::English)
        .count()
}
