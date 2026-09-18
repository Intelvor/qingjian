//! 形码查询：编码按前缀查码表，没有切分、没有整句；以及与拼音的混输。

use std::collections::HashSet;

use super::*;

impl Engine {
    /// 形码方案（五笔）的候选：编码打全的词排在同前缀的更长编码词前面，其余按词频与上下文。
    ///
    /// 拼音那一套在这里都不成立：编码不需要切分，没有简拼与模糊音，一处编辑的拼写纠错更不适用，
    /// 整句转换、中英混输与神经重排也没有可以展开的东西。反过来，译词标注、生词记录、输入日志、
    /// 用户选择学习与个人 n-gram 都按上屏的词工作，与拼音方案共用同一条路。
    pub(super) fn query_code(&self, keys: &str, rest: String, start: Instant) -> Query {
        let table = self.code.as_ref().expect("只在形码方案下调用");
        // 编码以外的字符（`no-way` 的 `-`）不是编码，交给原样上屏那条路
        if !keys.chars().all(|c| c.is_ascii_lowercase()) {
            return self.query_raw(keys, rest, start);
        }
        let parse = start.elapsed();

        let start = Instant::now();
        let log_total = (table.total_frequency() as f64).max(1.0).ln();
        let mut scored: Vec<Scored<'_>> = table
            .lookup(keys, MAX_CANDIDATES)
            .into_iter()
            .map(|hit| Scored {
                hit,
                // 编码是前缀匹配：没有「最后一个音节打完了」这回事，也不吃简拼、模糊音与敲错
                full_last: true,
                coverage: keys.len(),
                abbreviated: 0,
                weight: self.learner.weight(hit.text),
                penalty: 0.0,
            })
            .collect();
        let lookup = start.elapsed();

        // 同一段编码下选过的词优先，再按上下文得分（上一个上屏的词）；词频只在模型不认识时兜底
        let start = Instant::now();
        let letters = choice_key(keys, keys.len());
        ranking::rank(&mut scored, MAX_CANDIDATES, |item| {
            let choice = letters
                .get(..item.coverage)
                .map_or(0, |input| self.learner.choice_weight(input, item.hit.text));
            let log_prob = sentence::transition_log_prob(
                &*self.language_model,
                self.personal(),
                self.chain.context(),
                item.hit.text,
                sentence::fallback_log_prob(item.hit.frequency, log_total),
            );
            (choice, log_prob)
        });
        let rank = start.elapsed();
        let items: Vec<Candidate> = scored
            .into_iter()
            .map(|s| Candidate {
                text: s.hit.text.to_owned(),
                kind: CandidateKind::Code,
                // 编码不是拼音音节：候选窗按音节高亮的部分对形码没有意义，留空
                syllables: Vec::new(),
                reading: None,
                translation: None,
            })
            .collect();
        Query {
            // 形码没有切分：preedit 的显示串靠 `tail` 原样带出去（见 `Query::marked_text`）
            segmentations: Vec::new(),
            candidates: CandidateList { items },
            tail: keys.to_owned(),
            text: self.composition.text().to_owned(),
            cursor: self.composition.cursor(),
            rest,
            decoded_keys: false,
            typed_display: None,
            correction: None,
            timings: Timings {
                parse,
                lookup,
                rank,
            },
        }
    }

    /// 混输：形码与拼音两边都出候选，**形码在前**。
    ///
    /// 编码是精确的（四码定字），而混输的典型用法就是「主要用五笔，打不出的字才打拼音」，
    /// 所以形码命中的排前面。拼音那条路给不出解析时（`ggll` 切不成音节）不算失败——
    /// 整个查询就按形码的结果走，这也是「第 5 个字母起自动只剩拼音」的另一半：
    /// 五笔码最长 4 位，再往下敲形码本来就查不到东西。
    pub(super) fn query_mixed(
        &self,
        keys: &str,
        rest: String,
        start: Instant,
    ) -> Result<Query, ParseError> {
        let code = self.query_code(keys, rest.clone(), start);
        let mut query = match self.query_phonetic(keys, rest, start) {
            Ok(query) => query,
            Err(error) => {
                // 拼音读不出来，但形码有东西：把形码那条留着，出错只在两边都空时才算
                if code.candidates.items.is_empty() {
                    return Err(error);
                }
                return Ok(code);
            }
        };
        // 同一个词可能两边都命中（`gant` 既是编码又拼得出什么），按文本去重，形码那条留着
        let mut seen: HashSet<String> = query
            .candidates
            .items
            .iter()
            .map(|c| c.text.clone())
            .collect();
        let mut combined: Vec<Candidate> =
            Vec::with_capacity(code.candidates.items.len() + query.candidates.items.len());
        for candidate in code.candidates.items {
            if seen.insert(candidate.text.clone()) {
                combined.push(candidate);
            }
        }
        combined.extend(query.candidates.items);
        combined.truncate(MAX_CANDIDATES);
        query.candidates.items = combined;
        Ok(query)
    }
}
