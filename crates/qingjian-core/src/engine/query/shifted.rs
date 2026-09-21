//! 中文模式下 Shift 敲的大写字母：**大写字母永不参与拼音**，只作英文/专名。
//!
//! - `Nihao`：`N` 是英文，后面的 `ihao` 不是合法拼音 → 只出英文候选，不出「你好」；
//! - `Cpan`：`C` 是英文 + `pan` 拼「盘」→ 候选 `C盘`（上屏一次吃完整段）；
//! - `CyuyanheAIbiancheng`：多段大写 → 中英混排候选 `C语言和AI编程`。

use super::*;

/// 大写进组句时拆出来的英文与上屏用的大写头。
pub(super) struct ShiftedSplit {
    /// 逐字母命中且够常见时的领衔英文。
    pub leading: Option<Candidate>,
    /// 其余英文候选（含开头大写字母本身）。
    pub items: Vec<Candidate>,
    /// 开头连续大写（`C`）；仅当「去掉这些大写后的剩余」能切拼音时非空。
    pub leading_upper: String,
}

fn english_candidate(text: &str, syllables: Vec<String>) -> Candidate {
    Candidate {
        text: text.to_owned(),
        kind: CandidateKind::English,
        syllables,
        reading: None,
        translation: None,
        aux_code: None,
    }
}

/// 大写段 / 小写段拆分结果。
struct TypedSeg {
    text: String,
    is_upper: bool,
}

/// 按「连续大写 / 连续非大写」拆 `typed_scope`。
fn split_upper_lower(typed: &str) -> Vec<TypedSeg> {
    let mut segs = Vec::new();
    let mut buf = String::new();
    let mut is_upper = false;
    for c in typed.chars() {
        let up = c.is_ascii_uppercase();
        if buf.is_empty() {
            is_upper = up;
            buf.push(c);
        } else if up == is_upper {
            buf.push(c);
        } else {
            segs.push(TypedSeg {
                text: std::mem::take(&mut buf),
                is_upper,
            });
            is_upper = up;
            buf.push(c);
        }
    }
    if !buf.is_empty() {
        segs.push(TypedSeg {
            text: buf,
            is_upper,
        });
    }
    segs
}

impl Engine {
    /// 按 Shift 原样拆英文侧信息：`typed` 是 [`Composition::typed_scope`]。
    pub(super) fn shifted_split(&self, typed: &str) -> ShiftedSplit {
        let lists = self.english_lists();
        let mut items = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let mut push = |items: &mut Vec<Candidate>, text: &str, syllables: Vec<String>| {
            if seen.iter().any(|s| s == text) {
                return;
            }
            seen.push(text.to_owned());
            items.push(english_candidate(text, syllables));
        };

        let lower = typed.to_ascii_lowercase();
        let first_upper = typed
            .char_indices()
            .find(|(_, c)| c.is_ascii_uppercase())
            .map(|(i, _)| i);
        let leading_run: String = typed
            .chars()
            .take_while(|c| c.is_ascii_uppercase())
            .collect();

        // 开头大写且剩余能切拼音时，上屏要拼大写字面（Cpan → C盘）
        let leading_upper = if first_upper == Some(0) && !leading_run.is_empty() {
            let after_lower: String = typed
                .chars()
                .skip_while(|c| c.is_ascii_uppercase())
                .collect::<String>()
                .to_ascii_lowercase();
            if !after_lower.is_empty() && parser::segment(&after_lower).is_ok() {
                leading_run.clone()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // 整段小写命中英文词表：逐字母相同且常见才领衔
        let mut leading = None;
        if let Some(word) = lists.iter().find_map(|words| words.get(&lower)) {
            let frequency = lists.iter().find_map(|words| words.frequency(&lower));
            let exact_common = frequency.is_some_and(|f| f >= ENGLISH_FIRST_MIN_ZIPF);
            if exact_common {
                leading = Some(english_candidate(word, Vec::new()));
            } else {
                push(&mut items, word, Vec::new());
            }
        }

        // 从作用域开头拼最长英文前缀
        if leading.is_none()
            && items.is_empty()
            && (first_upper == Some(0) || first_upper.is_none())
        {
            let from = lower.as_str();
            let mut best: Option<(usize, &str)> = None;
            for len in (MIN_ENGLISH_WORD_LETTERS..=from.len()).rev() {
                if let Some(word) = lists.iter().find_map(|words| words.get(&from[..len])) {
                    best = Some((len, word));
                    break;
                }
            }
            if let Some((len, word)) = best {
                push(&mut items, word, vec![from[..len].to_owned()]);
            } else if from.len() >= MIN_COMPLETION_LETTERS {
                for word in lists
                    .iter()
                    .flat_map(|words| words.complete(from, ENGLISH_COMPLETIONS))
                    .take(ENGLISH_COMPLETIONS)
                {
                    push(&mut items, word, vec![from.to_owned()]);
                }
            }
        }

        // 开头大写字母本身作为英文候选（`Nihao` 的 `N`、`Cpan` 的 `C`）：上屏只吃这一段
        if !leading_run.is_empty()
            && leading.is_none()
            && !items.iter().any(|c| c.text == leading_run)
        {
            push(
                &mut items,
                &leading_run,
                vec![leading_run.to_ascii_lowercase()],
            );
        }

        ShiftedSplit {
            leading,
            items,
            leading_upper,
        }
    }

    /// 普通拼音查询跑完后，合并 Shift 大写带来的英文与「首字母大写 + 剩余拼音」候选。
    ///
    /// **大小写坚决分开**：大写字母永不参与拼音（`Nihao` 不出「你好」）——普通路径里
    /// 消耗到大写位置的中文 / 整句一律丢掉。**模式键只看整段的第一个字母**（`Cui` 不进 u 模式），
    /// 第二个及以后的字母不做模式匹配，但仍按大小写：小写走拼音，大写只作英文 / 混排字面。
    pub(super) fn merge_shifted_extras(&self, items: &mut Vec<Candidate>, typed: &str) {
        let split = self.shifted_split(typed);
        // 大写字母不进拼音：消耗到大写位置的中文 / 整句清掉
        let scope = self.composition.scope();
        let shifted: Vec<bool> = typed
            .chars()
            .zip(scope.chars())
            .map(|(t, _)| t.is_ascii_uppercase())
            .collect();
        items.retain(|c| {
            // 有大写时 emoji 也按整段小写拼音命中（Nihao→👋你好）：大小写分开，一并丢掉
            if matches!(c.kind, CandidateKind::Emoji) {
                return !shifted.iter().any(|s| *s);
            }
            if !matches!(c.kind, CandidateKind::Chinese | CandidateKind::Sentence) {
                return true;
            }
            let (consumed, _) = self.consumed_by(c);
            let limit = scope[..consumed.min(scope.len())].chars().count();
            !shifted.iter().take(limit).any(|s| *s)
        });
        if let Some(leading) = split.leading
            && !items.iter().any(|c| c.text == leading.text)
        {
            items.insert(0, leading);
        }
        // 首字母大写 + 剩余能切拼音（Cpan）：丢掉吃不掉前面大写字母的裸词
        if !split.leading_upper.is_empty() {
            let head = split.leading_upper.to_ascii_lowercase();
            items.retain(|c| {
                c.kind != CandidateKind::Chinese
                    || c.syllables.first().map(String::as_str) == Some(head.as_str())
            });
        }
        let mut extra = Vec::new();
        // 混排：① 敲了多段/长段大写（AIhenhaoyong）；② 只大写了首字母但整段小写前缀命中英文词
        // （Aihenhaoyong → 词表里 ai=AI + henhaoyong → AI很好用），避免候选只剩英文甚至空列表。
        let mixed = self
            .build_mixed_candidate(typed)
            .or_else(|| self.mixed_from_english_prefix(typed));
        if mixed.is_none() && !split.leading_upper.is_empty() {
            let after_lower: String = typed
                .chars()
                .skip_while(|c| c.is_ascii_uppercase())
                .collect::<String>()
                .to_ascii_lowercase();
            if !after_lower.is_empty() && parser::segment(&after_lower).is_ok() {
                if let Some((text, syllables)) = self.sentence_for_pinyin(&after_lower) {
                    extra.push(Candidate {
                        text,
                        kind: CandidateKind::Chinese,
                        syllables,
                        reading: None,
                        translation: None,
                        aux_code: None,
                    });
                }
                self.lookup_pinyin_words(&after_lower, &mut extra);
                let head = split.leading_upper.to_ascii_lowercase();
                for item in extra.iter_mut() {
                    if item.kind != CandidateKind::Chinese {
                        continue;
                    }
                    item.text = format!("{}{}", split.leading_upper, item.text);
                    let mut syllables = vec![head.clone()];
                    syllables.extend(item.syllables.iter().cloned());
                    item.syllables = syllables;
                }
            }
        }
        if let Some(mixed) = mixed {
            extra.push(mixed);
        }
        for cand in extra {
            if !items.iter().any(|c| c.text == cand.text) {
                items.push(cand);
            }
        }
        for cand in split.items {
            if !items.iter().any(|c| c.text == cand.text) {
                items.push(cand);
            }
        }
        if let Some(pos) = items.iter().position(|c| c.kind == CandidateKind::Mixed) {
            let mixed = items.remove(pos);
            items.insert(0, mixed);
        }
    }

    /// 把 `typed_scope` 按大写段 / 小写拼音段拆开，拼成中英混排候选。
    ///
    /// `CyuyanheAIbiancheng` → `C` + `语言和` + `AI` + `编程` = `C语言和AI编程`
    /// syllables 逐段覆盖整段 scope（大写段用小写音节），上屏一次吃完整段。
    ///
    /// 只在**至少两段大写**（或大写段 ≥2 字母，如 `AI`）时生成，
    /// 单字母开头且剩余是合法拼音（`Nihao`）不走这条路，避免误触 Caps 出 `N你好`。
    fn build_mixed_candidate(&self, typed: &str) -> Option<Candidate> {
        let segments = split_upper_lower(typed);
        // 至少两段大写，或有一段 ≥2 字母的大写（AI / API），才算混排意图
        let upper_runs = segments.iter().filter(|s| s.is_upper).count();
        let has_long_upper = segments
            .iter()
            .any(|s| s.is_upper && s.text.chars().count() >= 2);
        if upper_runs < 2 && !has_long_upper {
            return None;
        }
        let mut text = String::new();
        let mut syllables = Vec::new();
        let mut has_chinese = false;
        for seg in &segments {
            if seg.is_upper {
                text.push_str(&seg.text);
                syllables.push(seg.text.to_ascii_lowercase());
                continue;
            }
            let lower = seg.text.to_ascii_lowercase();
            if lower.is_empty() {
                continue;
            }
            // 段内音节数（最优切分）：只有覆盖整段的读法才能进混排，否则整段上屏会对不齐
            let seg_syllables = segment_longest_prefix(&lower)
                .ok()
                .and_then(|(segs, _)| segs.first().map(|s| s.syllables.len()))
                .unwrap_or(0);
            let mut words = Vec::new();
            self.lookup_pinyin_words(&lower, &mut words);
            let full_word = words.into_iter().find(|c| {
                c.kind == CandidateKind::Chinese
                    && seg_syllables > 0
                    && c.syllables.len() == seg_syllables
            });
            // 覆盖整段的词优先（编程）；没有就整句（yuyanhe → 语言和）
            let chosen = match full_word {
                Some(word) => Some((word.text, word.syllables)),
                None => self.sentence_for_pinyin(&lower),
            };
            match chosen {
                Some((hit_text, hit_syllables)) => {
                    text.push_str(&hit_text);
                    syllables.extend(hit_syllables);
                    has_chinese = true;
                }
                None => {
                    // 查不出覆盖整段的读法：原样保留这一段，保证上屏能吃完整段
                    text.push_str(&seg.text);
                    syllables.push(lower);
                }
            }
        }
        if !has_chinese || text.is_empty() {
            return None;
        }
        // 音节必须覆盖整段字母，否则上屏吃不完全、会留下残段
        let target: String = typed
            .chars()
            .filter(char::is_ascii_alphabetic)
            .collect::<String>()
            .to_ascii_lowercase();
        if syllables.concat() != target {
            return None;
        }
        Some(Candidate {
            text,
            kind: CandidateKind::Mixed,
            syllables,
            reading: None,
            translation: None,
            aux_code: None,
        })
    }

    /// 只大写了英文词的第一个字母（`Aihenhaoyong`）时：按**整段小写**在英文词表里找最长前缀
    /// （`ai` → `AI`），后半截仍按拼音出中文，拼成 `AI很好用`。词表里没有对应英文前缀、或后半截
    /// 切不成拼音时返回 `None`（`Nihao` 不会变成 `N你好`）。
    fn mixed_from_english_prefix(&self, typed: &str) -> Option<Candidate> {
        let lower: String = typed
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_ascii_lowercase();
        if lower.len() < 3 {
            return None;
        }
        let lists = self.english_lists();
        if lists.is_empty() {
            return None;
        }
        // 从长到短找英文词前缀：须够常见（与英文领衔同一门槛），后半截至少两个音节且能当拼音。
        // 否则 `Nihao` 会拼出 `NIH奥` 这类生造混排。
        let mut found: Option<(usize, String)> = None;
        for len in (2..lower.len()).rev() {
            let rest = &lower[len..];
            if rest.is_empty() {
                continue;
            }
            // 个人英文词表在前，次数往往很小（AI 选过 7 次）；门槛要按**各表里的最高词频**判，
            // 否则产品表里 Zipf 4230 的 AI 会被个人表的 7 挡掉，`Aihenhaoyong` 拼不出「AI很好用」。
            let freq = lists
                .iter()
                .filter_map(|words| words.frequency(&lower[..len]))
                .max();
            if freq.is_none_or(|f| f < ENGLISH_FIRST_MIN_ZIPF) {
                continue;
            }
            let Some(word) = lists.iter().find_map(|words| words.get(&lower[..len])) else {
                continue;
            };
            let rest_syllables = segment_longest_prefix(rest)
                .ok()
                .and_then(|(segs, _)| segs.first().map(|s| s.syllables.len()))
                .unwrap_or(0);
            if rest_syllables < 2 {
                continue;
            }
            if parser::segment(rest).is_err() && self.sentence_for_pinyin(rest).is_none() {
                continue;
            }
            found = Some((len, word.to_owned()));
            break;
        }
        let (en_len, en_text) = found?;
        let rest = &lower[en_len..];
        let mut words = Vec::new();
        self.lookup_pinyin_words(rest, &mut words);
        let seg_syllables = segment_longest_prefix(rest)
            .ok()
            .and_then(|(segs, _)| segs.first().map(|s| s.syllables.len()))
            .unwrap_or(0);
        let full_word = words.into_iter().find(|c| {
            c.kind == CandidateKind::Chinese
                && seg_syllables > 0
                && c.syllables.len() == seg_syllables
        });
        let (cn_text, cn_syllables) = match full_word {
            Some(word) => (word.text, word.syllables),
            None => self.sentence_for_pinyin(rest)?,
        };
        let mut syllables = vec![lower[..en_len].to_owned()];
        syllables.extend(cn_syllables);
        if syllables.concat() != lower {
            return None;
        }
        Some(Candidate {
            text: format!("{en_text}{cn_text}"),
            kind: CandidateKind::Mixed,
            syllables,
            reading: None,
            translation: None,
            aux_code: None,
        })
    }

    /// 对一段独立拼音跑整句转换，返回 (文字, 音节)。
    /// 只收**音节数与切分一致**、且不脑补超长文本的整句。
    fn sentence_for_pinyin(&self, pinyin: &str) -> Option<(String, Vec<String>)> {
        let (segmentations, _tail) = segment_longest_prefix(pinyin).ok()?;
        let best = segmentations.first()?;
        if best.syllables.is_empty() {
            return None;
        }
        let patterns = best.patterns();
        let conversion = self.convert_sentence(&patterns, false)?;
        if conversion.has_placeholder() {
            return None;
        }
        // 音节数要贴住切分，否则不算这段拼音的整句
        if conversion.syllables.len() != best.syllables.len() {
            return None;
        }
        // 整句文本的拼音长度明显超出输入：模型脑补，丢弃
        let input_letters = pinyin.chars().filter(|c| *c != '\'').count();
        let sent_letters: usize = conversion.syllables.iter().map(|s| s.len()).sum();
        if sent_letters > input_letters + 4 {
            return None;
        }
        // 音节必须与这段字母逐字对上，否则上屏对不齐
        if conversion.syllables.concat() != best.joined("") {
            return None;
        }
        Some((conversion.text, conversion.syllables))
    }

    /// 词级候选（含前缀词）：给大写路径补查拼音用。
    fn lookup_pinyin_words(&self, prefix: &str, out: &mut Vec<Candidate>) {
        let Ok((segmentations, _tail)) = segment_longest_prefix(prefix) else {
            return;
        };
        let mut scored = Vec::new();
        for (seg_i, segmentation) in segmentations.iter().enumerate() {
            let mut patterns = segmentation.patterns();
            let count = patterns.len();
            if count == 0 {
                continue;
            }
            let last = &segmentation.syllables[count - 1];
            if last.complete && parser::is_syllable_prefix(&last.text) {
                patterns[count - 1].complete = false;
            }
            let expanded = self.fuzzy.expand(&patterns);
            let positions = expanded.positions();
            let abbreviated = abbreviated_count(&patterns);
            for hit in self.lookup_all(&positions) {
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
            if seg_i == 0 {
                for prefix_len in (1..count).rev() {
                    let prefix_letters: usize =
                        patterns[..prefix_len].iter().map(|x| x.text.len()).sum();
                    for hit in self.lookup_exact_all(&positions[..prefix_len]) {
                        scored.push(Scored {
                            hit: Match {
                                exact: false,
                                ..hit
                            },
                            full_last: true,
                            coverage: prefix_letters,
                            abbreviated: abbreviated_count(&patterns[..prefix_len]),
                            weight: self.learner.weight(hit.text),
                            penalty: expanded.penalty(hit.syllables()),
                        });
                    }
                }
            }
        }
        let log_total = (self.total_frequency() as f64).max(1.0).ln();
        let input_letters = prefix.chars().filter(|c| *c != '\'').count();
        ranking::rank(&mut scored, MAX_CANDIDATES, |item| {
            let hit = &item.hit;
            let log_prob = sentence::transition_log_prob(
                &*self.language_model,
                self.personal(),
                self.chain.context(),
                hit.text,
                sentence::fallback_log_prob(hit.frequency, log_total),
            );
            let hit_letters: usize = hit.syllables().map(|s| s.len()).sum();
            let log_prob = log_prob - ranking::excess_pinyin_penalty(hit_letters, input_letters);
            (item.weight, log_prob)
        });
        out.extend(scored.into_iter().map(|s| Candidate {
            text: s.hit.text.to_owned(),
            kind: CandidateKind::Chinese,
            syllables: s.hit.syllables().map(str::to_owned).collect(),
            reading: None,
            translation: None,
            aux_code: None,
        }));
    }
}
