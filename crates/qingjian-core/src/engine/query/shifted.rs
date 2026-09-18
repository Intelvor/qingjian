//! 中文模式下 Shift 敲的大写字母：不当拼音小写去匹配。
//!
//! 大写是「这是英文 / 专有名词」的信号：先在整段与从作用域开头起的大写前缀上拼英文词表，
//! 拼不出就孤立该字母；拼音只吃**大写之前的纯小写前缀**，大写本身不进切分。

use super::*;

/// 大写进组句时拆出来的候选与可拼的拼音前缀。
pub(super) struct ShiftedSplit {
    /// 逐字母命中且够常见时的领衔英文（可排第一）；否则 `None`。
    pub leading: Option<Candidate>,
    /// 其余英文候选（不常见的精确词、前缀命中、孤立大写字母）。
    pub items: Vec<Candidate>,
    /// 大写之前的纯小写前缀；空串表示没有可拼的拼音。
    pub pinyin_prefix: String,
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
        let pinyin_prefix = match first_upper {
            Some(index) => typed[..index].to_owned(),
            None => typed.to_owned(),
        };

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

        // ② 从作用域开头起拼最长英文前缀（大写在开头时就是从大写起）；
        //    只收从 0 起的命中，上屏按音节消耗对得上。中途大写留给下一轮。
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

        // ③ 拼不出英文就孤立大写字母（词表里有单字母写法的用词表，如 I）
        if leading.is_none()
            && items.is_empty()
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
        }
    }

    /// 大写前纯小写前缀的词级候选：正常查词 + 排序，不套纠错（大写已标出边界）。
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
            (choice, log_prob)
        });
        out.extend(scored.iter().map(|s| chinese_candidate(&s.hit)));
    }

    /// 大写进组句的查询：英文在前，大写前的纯小写前缀再走正常拼音。
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
