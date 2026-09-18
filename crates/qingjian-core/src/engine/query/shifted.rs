//! 中文模式下 Shift 敲的大写字母：不当拼音小写去匹配。
//!
//! 大写是「这是英文 / 专有名词」的信号：先在整段与从作用域开头起的大写前缀上拼英文词表，
//! 拼不出就孤立该字母。
//!
//! **拼音前缀**（2026-09-19 修冻结）：
//! - 大写在中间：只吃大写之前的纯小写；
//! - 大写在**开头**：先看去掉开头大写后的剩余能不能当拼音（`Cpan`→`pan`，避免又拼回 C盘）；
//!   剩余切不开时退回**整段小写**（`Nihao`→`nihao`）——首字母大写多半是误触或英文起头，
//!   否则 pinyin_prefix 恒为空，后面的字母和候选再也不更新，看起来像输入框冻住。

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
        return (typed[..first_upper].to_owned(), String::new());
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
        (String::new(), leading_upper)
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

    /// 拼音前缀的词级候选：正常查词 + 排序。
    fn lookup_pinyin_words(&self, prefix: &str, out: &mut Vec<Candidate>) {
        let Ok(segmentations) = segment_longest_prefix(prefix) else {
            return;
        };
        let (segmentations, _tail) = segmentations;
        let mut scored = Vec::new();
        for segmentation in &segmentations {
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
        if !split.pinyin_prefix.is_empty() {
            self.lookup_pinyin_words(&split.pinyin_prefix, &mut items);
            // 开头大写 + 剩余拼音：中文候选要带上大写字面，上屏一次吃完整段
            //（`Cpan` 选「盘」→ 上屏 `C盘` 并清空，而不是只上「盘」留下 `c`）
            if !split.leading_upper.is_empty() {
                let head_lower = split.leading_upper.to_ascii_lowercase();
                for item in items.iter_mut() {
                    if item.kind != CandidateKind::Chinese {
                        continue;
                    }
                    if item.syllables.first().map(String::as_str) == Some(head_lower.as_str()) {
                        continue;
                    }
                    item.text = format!("{}{}", split.leading_upper, item.text);
                    let mut syllables = vec![head_lower.clone()];
                    syllables.extend(item.syllables.iter().cloned());
                    item.syllables = syllables;
                }
            }
        }
        items.extend(split.items);
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
