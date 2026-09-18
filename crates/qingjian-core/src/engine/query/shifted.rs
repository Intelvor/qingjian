//! 中文模式下 Shift 敲的大写字母：拼音仍按整段小写走普通查询，大写只影响英文合并与上屏字面。
//!
//! - 普通路径：`scope()`（全小写）做查词 / 整句，**中途大写（`AI`）后面的拼音不会断**；
//! - `merge_shifted_extras`：合并英文候选；开头大写且剩余能切拼音时（`Cpan`）给中文候选拼上大写字面 → `C盘`。

use super::*;

/// 大写进组句时拆出来的英文与上屏用的大写头。
pub(super) struct ShiftedSplit {
    /// 逐字母命中且够常见时的领衔英文。
    pub leading: Option<Candidate>,
    /// 其余英文候选。
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
    }
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

        // 开头大写且剩余能切拼音时，上屏要拼大写字面（Cpan → C盘）
        let leading_upper = match first_upper {
            Some(0) => {
                let upper: String = typed
                    .chars()
                    .take_while(|c| c.is_ascii_uppercase())
                    .collect();
                let after: String = typed
                    .chars()
                    .skip_while(|c| c.is_ascii_uppercase())
                    .collect();
                let after_lower = after.to_ascii_lowercase();
                if !after_lower.is_empty() && parser::segment(&after_lower).is_ok() {
                    upper
                } else {
                    String::new()
                }
            }
            _ => String::new(),
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

        ShiftedSplit {
            leading,
            items,
            leading_upper,
        }
    }

    /// 普通拼音查询跑完后，合并 Shift 大写带来的英文与大写字面。
    pub(super) fn merge_shifted_extras(&self, items: &mut Vec<Candidate>, typed: &str) {
        let split = self.shifted_split(typed);
        if let Some(leading) = split.leading
            && !items.iter().any(|c| c.text == leading.text)
        {
            items.insert(0, leading);
        }
        // 开头大写且剩余可拼音：先丢掉整段小写查出来、吃不掉前面字母的裸词（Cpan 的「盘」）
        if !split.leading_upper.is_empty() {
            let head = split.leading_upper.to_ascii_lowercase();
            items.retain(|c| {
                c.kind != CandidateKind::Chinese
                    || c.syllables.first().map(String::as_str) == Some(head.as_str())
            });
        }
        let lower = typed.to_ascii_lowercase();
        let after_upper: String = typed
            .chars()
            .skip_while(|c| c.is_ascii_uppercase())
            .collect::<String>()
            .to_ascii_lowercase();
        let stripped: String = typed
            .chars()
            .filter(|c| !c.is_ascii_uppercase())
            .collect::<String>()
            .to_ascii_lowercase();
        let mut extra = Vec::new();
        if !split.leading_upper.is_empty() && !after_upper.is_empty() {
            self.lookup_pinyin_words(&after_upper, &mut extra);
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
        if stripped != lower && !stripped.is_empty() && stripped != after_upper {
            self.lookup_pinyin_words(&stripped, &mut extra);
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
        }));
    }
}
