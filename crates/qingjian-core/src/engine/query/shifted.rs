//! 中文模式下 Shift 敲的大写字母：不当拼音小写去匹配。
//!
//! 大写是「这是英文 / 专有名词」的信号：英文侧先拼词表；**拼音侧始终用整段小写**，
//! 夹在中间的大写（`AI`）后面的字母仍要参与切分。
//!
//! - 开头大写且剩余能切拼音（`Cpan`）：拼音只用剩余，大写字面拼进中文候选 → `C盘`；
//! - 开头大写剩余切不开（`Nihao`）：整段小写当拼音 → 你好；
//! - 中途大写（`wo'he'AIbiancheng`）：整段小写当拼音 → 我和 / 编程 等，不再在 AI 处断掉。

use super::*;

/// 大写进组句时拆出来的候选与可拼的拼音前缀。
pub(super) struct ShiftedSplit {
    /// 逐字母命中且够常见时的领衔英文（可排第一）；否则 `None`。
    pub leading: Option<Candidate>,
    /// 其余英文候选（不常见的精确词、前缀命中、孤立大写字母）。
    pub items: Vec<Candidate>,
    /// 用来走拼音词级查询的前缀；空串表示本拍没有拼音候选。
    pub pinyin_prefix: String,
    /// 开头连续大写（原样，如 `C`）；**仅当**拼音前缀是「去掉这些大写之后的剩余」时非空。
    /// 上屏时要和拼音候选拼在一起（`C`+`盘`→`C盘`），否则只上「盘」会留下 `c` 清不掉。
    pub leading_upper: String,
}

fn english_candidate(text: &str, syllables: Vec<String>) -> Candidate {
    Candidate {
        text: text.to_owned(),
        kind: CandidateKind::English,
        syllables,
        reading: None,
        translation: None,
    }
}

fn chinese_candidate(hit: &Match<'_>) -> Candidate {
    Candidate {
        text: hit.text.to_owned(),
        kind: CandidateKind::Chinese,
        syllables: hit.syllables().map(str::to_owned).collect(),
        reading: None,
        translation: None,
    }
}

/// 返回 `(拼音前缀, 开头连续大写)`。
///
/// - 无大写：整段小写，大写头为空；
/// - 大写在中间：前缀 = 大写之前，大写头为空（中途大写留给下一轮）；
/// - 大写在开头、剩余能切拼音：前缀 = 剩余小写，大写头 = 开头大写（上屏要拼上）；
/// - 大写在开头、剩余切不开：前缀 = 整段小写（误触 Caps），大写头为空。
fn shifted_pinyin_prefix(typed: &str, lower: &str) -> (String, String) {
    let Some(first_upper) = typed
        .char_indices()
        .find(|(_, c)| c.is_ascii_uppercase())
        .map(|(i, _)| i)
    else {
        return (lower.to_owned(), String::new());
    };
    if first_upper > 0 {
        // 中途大写（wo'he'AIbiancheng / nihaoJava）：
        // 优先「去掉大写字面后的全部小写字母」仍要能切后面的拼音；
        // 切不开再退回大写之前的纯小写（nihao|Java → nihao）。
        let stripped: String = typed
            .chars()
            .filter(|c| !c.is_ascii_uppercase())
            .collect::<String>()
            .to_ascii_lowercase();
        if parser::segment(&stripped).is_ok() {
            return (stripped, String::new());
        }
        let before = typed[..first_upper].to_ascii_lowercase();
        if parser::segment(&before).is_ok() {
            return (before, String::new());
        }
        return (lower.to_owned(), String::new());
    }
    let leading_upper: String = typed
        .chars()
        .take_while(|c| c.is_ascii_uppercase())
        .collect();
    let after: String = typed
        .chars()
        .skip_while(|c| c.is_ascii_uppercase())
        .collect();
    let after_lower = after.to_ascii_lowercase();
    if after_lower.is_empty() {
        (String::new(), String::new())
    } else if parser::segment(&after_lower).is_ok() {
        (after_lower, leading_upper)
    } else {
        (lower.to_owned(), String::new())
    }
}

impl Engine {
    /// 按 Shift 原样拆一段组句：`typed` 是 [`Composition::typed_scope`]。
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
        let (pinyin_prefix, leading_upper) = shifted_pinyin_prefix(typed, &lower);

        // ① 整段按小写查词表：逐字母相同且常见才领衔，否则英文跟在中文后面
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

        // ② 从作用域开头起拼最长英文前缀；只收从 0 起的命中
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

        // ③ 拼不出英文、拼音前缀也空时才孤立大写字母
        if leading.is_none()
            && items.is_empty()
            && pinyin_prefix.is_empty()
            && let Some(index) = first_upper
        {
            let letter = typed[index..].chars().next().expect("uppercase char");
            let code = letter.to_ascii_lowercase().to_string();
            let owned;
            let text = match lists.iter().find_map(|words| words.get(&code)) {
                Some(word) => word,
                None => {
                    owned = letter.to_string();
                    owned.as_str()
                }
            };
            push(&mut items, text, vec![code]);
        }

        ShiftedSplit {
            leading,
            items,
            pinyin_prefix,
            leading_upper,
        }
    }

    /// 拼音前缀的词级候选：正常查词 + 排序；**含前缀词**（`wohebiancheng` → 我和 / 编程）。
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
            // 前缀词：整段切开时也要能出「我和」「编程」这类盖住前半段的词（只在最优切分上做）
            if seg_i == 0 {
                for prefix_len in (1..count).rev() {
                    let p = &patterns[..prefix_len];
                    let prefix_letters: usize = p.iter().map(|x| x.text.len()).sum();
                    for hit in self.lookup_exact_all(&positions[..prefix_len]) {
                        scored.push(Scored {
                            hit: Match {
                                exact: false,
                                ..hit
                            },
                            full_last: true,
                            coverage: prefix_letters,
                            abbreviated: abbreviated_count(p),
                            weight: self.learner.weight(hit.text),
                            penalty: expanded.penalty(hit.syllables()),
                        });
                    }
                }
            }
        }
        let log_total = (self.total_frequency() as f64).max(1.0).ln();
        let scope = prefix;
        let letters = choice_key(scope, scope.len());
        let input_letters = scope.chars().filter(|c| *c != '\'').count();
        ranking::rank(&mut scored, MAX_CANDIDATES, |item| {
            let hit = &item.hit;
            let covered = item.coverage;
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
            let hit_letters: usize = hit.syllables().map(|s| s.len()).sum();
            let log_prob = log_prob - ranking::excess_pinyin_penalty(hit_letters, input_letters);
            (choice, log_prob)
        });
        out.extend(scored.iter().map(|s| chinese_candidate(&s.hit)));
    }

    /// 大写进组句的查询：英文领衔（若有），拼音前缀出中文，其余英文在后。
    pub(super) fn query_shifted(
        &self,
        typed: &str,
        rest: String,
        start: Instant,
    ) -> Result<Query, ParseError> {
        let split = self.shifted_split(typed);
        let mut items = Vec::new();
        if let Some(leading) = split.leading {
            items.push(leading);
        }
        // 拼音源合并查：大写拆出来的前缀 + 去大写后的字母流 + 整段小写
        //（woheAIbiancheng 只查「wohe」会在 AI 处断掉，biancheng 整段丢失）
        let lower = typed.to_ascii_lowercase();
        let stripped: String = typed
            .chars()
            .filter(|c| !c.is_ascii_uppercase())
            .collect::<String>()
            .to_ascii_lowercase();
        let mut chinese = Vec::new();
        for src in [&split.pinyin_prefix, &stripped, &lower] {
            if src.is_empty() {
                continue;
            }
            self.lookup_pinyin_words(src, &mut chinese);
        }
        // 去重
        let mut seen = std::collections::HashSet::new();
        chinese.retain(|c| seen.insert(c.text.clone()));
        if !split.pinyin_prefix.is_empty() && !split.leading_upper.is_empty() {
            let head_lower = split.leading_upper.to_ascii_lowercase();
            for item in chinese.iter_mut() {
                if item.syllables.first().map(String::as_str) == Some(head_lower.as_str()) {
                    continue;
                }
                item.text = format!("{}{}", split.leading_upper, item.text);
                let mut syllables = vec![head_lower.clone()];
                syllables.extend(item.syllables.iter().cloned());
                item.syllables = syllables;
            }
        }
        items.extend(chinese);
        items.extend(split.items.clone());
        // 整句：对拼音源也跑 plain_sentence（wohebiancheng → 我和编程）
        for src in [&stripped, &lower, &split.pinyin_prefix] {
            if src.is_empty() {
                continue;
            }
            if let Ok((segs, _)) = segment_longest_prefix(src)
                && !segs.is_empty()
            {
                if let Some((plain, _)) = self.plain_sentence(&mut items, &segs, true) {
                    let pos = Self::sentence_insert_position(&plain, &items);
                    items.insert(pos, plain);
                }
                break;
            }
        }
        let segmentations = if split.pinyin_prefix.is_empty() {
            Vec::new()
        } else {
            segment_longest_prefix(&split.pinyin_prefix)
                .map(|(segs, _)| segs)
                .unwrap_or_default()
        };
        let tail = split.pinyin_prefix.clone();
        let typed_display = join_marked_typed(typed, &segmentations, &tail);
        Ok(Query {
            segmentations,
            candidates: CandidateList { items },
            tail,
            text: self.composition.text().to_owned(),
            cursor: self.composition.cursor(),
            rest,
            decoded_keys: self.shuangpin.is_some() || self.zhuyin,
            typed_display: Some(typed_display),
            correction: None,
            timings: Timings {
                parse: start.elapsed(),
                lookup: Duration::ZERO,
                rank: start.elapsed(),
            },
        })
    }
}
