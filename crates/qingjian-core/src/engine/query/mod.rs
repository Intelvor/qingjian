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
pub(super) use result::join_marked_typed;
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
        let decoded = self.decode(keys);
        // Shift 大写进组句：不当拼音小写去匹配，先拼英文 / 孤立字母，拼音只吃大写前的纯小写前缀
        if decoded.is_none() && self.composition.has_shifted() {
            return self.query_shifted(&self.composition.typed_scope(), rest, start);
        }
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
        let rank = start.elapsed();

        // 按头段算时英文尾段不参与拼音候选，显示上跟在切分后面：`wo'xiang'xue'hao'rust`
        let tail = english_tail
            .as_ref()
            .filter(|_| head_wins)
            .map_or(tail, |t| &keys[t.head_len..]);
        let typed_display = decoded.as_ref().map(|d| d.marked()).or_else(|| {
            // 中文模式下 Shift 敲的大写：匹配按小写算，拼音行仍按敲的样子显示（`Cpan`）
            (correction.is_none() && self.composition.has_shifted())
                .then(|| join_marked_typed(&self.composition.typed_scope(), &segmentations, tail))
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
                    && let Some(plain) =
                        self.plain_sentence(items, std::slice::from_ref(&full), typos)
                {
                    let position = items.len().min(1);
                    items.insert(position, plain);
                }
            }
            _ => {
                if let Some(plain) = self.plain_sentence(items, segmentations, typos) {
                    let position = leading_english(items);
                    items.insert(position, plain);
                }
            }
        }
    }

    /// 最优切分之外的切分试几条。太靠后的切分离最长切分太远（多是拆碎音节的读法），试了只是白花时间。
    const ALTERNATIVE_SEGMENTATIONS: usize = 4;

    /// 在最优切分之外找一条「不用敲错边、也没有占位音节」的整句读法，给 [`Self::plain_sentence`] 兜底。
    ///
    /// **只收与最优切分音节数相同的读法**（`zhenandaobushi` 的 zhe'nan… 与 zhen'an… 都是 5 段）。
    /// 简拼拆出来的一长串单字（`zonghu` 的 z…o…n…g…hu → 在哦那个和）音节数对不上，不是「同一句话的另一种切法」，
    /// 拿它当整句会顶掉 总会 这类正常词候选。
    fn plain_alternative(&self, segmentations: &[Segmentation], typos: bool) -> Option<Conversion> {
        let best_len = segmentations.first()?.syllables.len();
        segmentations
            .iter()
            .skip(1)
            .take(Self::ALTERNATIVE_SEGMENTATIONS)
            .find_map(|segmentation| {
                if segmentation.syllables.len() != best_len {
                    return None;
                }
                let patterns = segmentation.patterns();
                let inner_abbreviated = patterns
                    .iter()
                    .take(patterns.len().saturating_sub(1))
                    .any(|p| !p.complete);
                if inner_abbreviated {
                    return None;
                }
                let conversion = self.convert_sentence(&patterns, typos)?;
                (!conversion.altered() && !conversion.has_placeholder()).then_some(conversion)
            })
    }

    /// 整段拼音的整句候选：最优切分至少两个音节、且最优路径不止一个词时才有（空格上屏的就是它）。
    /// 整段本身就是词库里的词时不重复；有音节没转成字的不算句子。
    /// 词级候选里已有同文本同读音的候选时不出（那条留在词级排序给它的位置），同文本不同读音的从 `items` 里去掉。
    pub(super) fn plain_sentence(
        &self,
        items: &mut Vec<Candidate>,
        segmentations: &[Segmentation],
        typos: bool,
    ) -> Option<Candidate> {
        let best = segmentations.first()?;
        if best.syllables.len() < 2 {
            return None;
        }
        let mut conversion = self.convert_sentence(&best.patterns(), typos)?;
        // 不按原样读的路径（敲错边 / 模糊音）不许压过「敲的拼音本身就是一个词」：`jineng` 按 `jin eng` 切时
        // 词图里没有 技能，敲错边读出 近藤；`ceshi` 读出 的是。词级候选里有音节正好拼成整段输入的词时退回原样的路径
        if conversion.altered() {
            let letters = best.joined("");
            let spelled_exactly = items
                .iter()
                .any(|c| c.kind == CandidateKind::Chinese && c.syllables.concat() == letters);
            if spelled_exactly {
                conversion = self.convert_sentence(&best.patterns(), false)?;
            } else if let Some(alternative) = self.plain_alternative(segmentations, typos) {
                // 最优切分只能靠敲错边读出句子时，换别的切分试试：切分按最长音节优先，而最长的不一定对。
                // `zhenandaobushi` 的最长切分是 `zhen an …`，它读成「真难道不是」全靠 an → nan 的敲错边；
                // 而 `zhe nan …` 是原样的「这难道不是」。
                conversion = alternative;
            }
        }
        if conversion.has_placeholder() {
            return None;
        }
        // 整句读法的音节数要贴住引擎显示的那条切分；差一截的多半是简拼串成的一串单字
        // （`zonghu` → 在哦那个和），空格上屏会很怪，不进候选首位。
        if conversion.syllables.len() != best.syllables.len() {
            return None;
        }
        // 整段本来就是一个词时不出整句；但路径靠敲错变体把整段读成的一个词（`meiganxi` → 没关系）是噪声信道的判断，
        // 词级查询按原样查不到它，作为普通词候选插到最前。只读了一部分（末尾没打完的音节没算进去）的不插
        let kind = if conversion.word_count() >= 2 {
            CandidateKind::Sentence
        } else if conversion.altered() && conversion.syllables.len() == best.syllables.len() {
            CandidateKind::Chinese
        } else {
            return None;
        };
        // 词级候选里已经有同样的文本：读音也相同就是同一个候选，不重复插、词留在词级排序给它的位置
        //（先是 / 有的 这种整句恰好拼成一个词的，词级排序更可信）；读音不同的是按别的读音对上的词
        //（云端学来的错读音用户词 `我的 wo di` 靠敲错变体对上 `wode`），那条不是这个候选，去掉它，整句以正确读音顶上
        if let Some(index) = items.iter().position(|c| c.text == conversion.text) {
            if items[index].syllables == conversion.syllables {
                return None;
            }
            items.remove(index);
        }
        Some(Candidate {
            text: conversion.text,
            kind,
            syllables: conversion.syllables,
            reading: None,
            translation: None,
        })
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
