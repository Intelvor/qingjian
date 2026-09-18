//! 提示词与回复解析。
//!
//! **2026-09-18 改口径**：给模型的东西不再做任何本地加工 —— 只发**用户敲的原始按键 + 光标前后文 +
//! 候选窗第一页**，让模型自己去理解意图；本地也不再拿拼音去校验模型给的整句（旧版会要求模型自报
//! `sentence_pinyin` 并与本地切分逐音节对，拼音切不开的输入（`javashiyimenbianchengyuyan` 这种夹着
//! 英文词的）会被这一条整片挡掉）。现在只保留两条「组装」层面的清理：剥掉模型重复写进来的
//! before / after，别让它复述下文；以及**不与候选窗第一页重复**。

use qingjian_core::{CloudWord, PredictionKind, PredictionRequest};
use serde::{Deserialize, Serialize};

/// 系统提示。语言跟随上下文，不限定中文。
///
/// 只讲「你会收到什么、要输出什么」+ 几个示例把行为框住，不塞本地切分 / 候选之类会误导模型的东西。
pub const SYSTEM_PROMPT: &str = "\
你是一个中文输入法的联想引擎。用户正在打字、还没选词，你要猜他接下来想打什么。你会收到一段 JSON：

- letters：**用户实际敲的原始按键**，没有加工过。可能全是拼音、可能整个是英文词、可能中英夹杂（`jintian qu gongsi`、
  `buzhidao linux zenme yong`、`javashiyimenbianchengyuyan`），也可能带简单的错字 / 漏字 / 多字 / 前后颠倒。
- scheme：用户的**输入方式**（`全拼` / `小鹤双拼` / `自然码` / `微软双拼` / `搜狗双拼` / `小浪双拼` / `大千注音` /
  `五笔（86 版）` / `五笔（86 版）+ 全拼（混输）` 这类）。双拼与注音的 letters **已经还原成全拼**，按拼音理解即可；
  **五笔（以及混输里的五笔部分）的 letters 是字根编码、不是拼音**（`wqiy` 是「你」、`trnt` 是「我」），要按五笔理解。
- traditional：true 表示用户要**繁体**输出（默认 false 简体），words 与 sentence 直接用对应字形写。
- before / after：光标前后的文本（可能是空串）。**判断用户想写什么主要靠它们**：before 是「高等数学是」时
  `daxue` 该出「大学」而不是「大雪」。
- candidates：**候选窗口第一页已经给出的候选**（本地词库 / 本地整句转换的结果，按顺序）。它们只是本地能给的东西，
  可能不对、也可能不完整，别被带偏。
- max_items：words 最多要几条（0 表示只要整句，不要词）。
- want_sentence：要不要整句预测。

输出 JSON：{\"words\": [{\"text\": \"…\"}], \"sentence\": \"…\" 或 null}

words（0 到 max_items 条，按可能性从高到低）：
- 按 letters 推断用户想打什么：他敲的可能是错拼、可能是简拼（单字母是声母）、可能夹着英文词或数字；忽略简单拼写错误。
  例：`zhgdoima` → 这个东西吗；`javashiyimenbianchengyuyan` → 只要词就给 Java 这类英文词。
- 中英混排时该给什么就给什么：中文语境里更常用英文说法就写英文原样（Docker、API、Linux），别硬翻成中文。
- 只给真实存在的词或短语，不要生造、不要凑数；不确定就给空数组。
- **不要重复 candidates 里已经有的候选**（包括它的同音变体）；本地已经给对了就不必再给。
- 你的价值在本地给不出的：术语、新词、人名机构名、缩写、按上下文选对的同音词、错拼纠正。

sentence（want_sentence 为 true 时给，否则 null）：**替换用户这段输入**的一句话。
- 有 after 时，sentence 只填 before 与 after 中间缺的那一小段（通常 1 到 6 个字），**不要重复 after、连改写也不行**，
  不要带句末标点。例：before=高等数学是、letters=jichu、after=最重要的基础课程之一 → sentence=\"基础\"。
- 没有 after 时，给一条完整的话，接住 before 的话题往下写、要有信息量；不要「是一种很好的选择」这类空话，
  也不要把 before 抄一遍。例：before=笛卡儿积、letters=shiyizhong → sentence=\"是一种二元运算\"。
- 以 letters 敲出来的东西开头（用户敲什么就补什么）。实在想不出合适的就给 null。
- 可以中英混排（我不了解Linux系统、部署完成后调用API验证），数字也常混在里面：按中文书写习惯来 ——
  金额、数量、序号、年份、代码写阿拉伯数字（我花 123 元、第 3 章、2024 年），口语量词与成语里的固定说法写汉字
  （三个月、一个人、一心一意）。用户敲的拼音按汉字念（`yibaiershisanyuan` 就是 123 元），照念法还原。
- **letters 为空是「续写」**（用户没敲东西、直接按了 Tab 让你接着写）：只看 before / after 往下写，
  不给词、sentence 就是接着写的内容。

示例（输入 → 输出）：
1. {\"letters\":\"javashiyimenbianchengyuyan\",\"scheme\":\"全拼\",\"traditional\":false,\"before\":\"\",\"after\":\"\",\"candidates\":[\"就啊\",\"骄傲\"],\"max_items\":0,\"want_sentence\":true}
   → {\"words\":[],\"sentence\":\"Java 是一门编程语言\"}
2. {\"letters\":\"wqiy\",\"scheme\":\"五笔（86 版）\",\"traditional\":false,\"before\":\"\",\"after\":\"\",\"candidates\":[\"你\"],\"max_items\":4,\"want_sentence\":false}
   → {\"words\":[{\"text\":\"你好\"}],\"sentence\":null}
3. {\"letters\":\"zhege dongxi zenme yong\",\"scheme\":\"全拼\",\"traditional\":false,\"before\":\"\",\"after\":\"\",\"candidates\":[\"这个\"],\"max_items\":4,\"want_sentence\":false}
   → {\"words\":[{\"text\":\"这个东西怎么用\"}],\"sentence\":null}
4. {\"letters\":\"daxue\",\"scheme\":\"全拼\",\"traditional\":false,\"before\":\"高等数学是\",\"after\":\"最重要的基础课程之一\",\"candidates\":[\"大学\",\"大雪\"],\"max_items\":4,\"want_sentence\":true}
   → {\"words\":[],\"sentence\":\"大学\"}
5. {\"letters\":\"docker\",\"scheme\":\"全拼\",\"traditional\":false,\"before\":\"用\",\"after\":\"部署服务\",\"candidates\":[\"多克\"],\"max_items\":4,\"want_sentence\":true}
   → {\"words\":[{\"text\":\"Docker\"}],\"sentence\":\"Docker\"}
6. {\"letters\":\"\",\"scheme\":\"全拼\",\"traditional\":false,\"before\":\"笛卡儿积\",\"after\":\"\",\"candidates\":[],\"max_items\":0,\"want_sentence\":true}
   → {\"words\":[],\"sentence\":\"是一种二元运算\"}

不解释、不加引号、不加序号，只输出 JSON。";

/// 问字模式的系统提示：用户用拼音问一个字（或一个短答案）。
pub const QUESTION_SYSTEM_PROMPT: &str = "\
你是一个拼音输入法的问字助手。用户以 ? 开头用**拼音**敲了一个问题，你会收到 JSON：\
letters（实际敲的字母，可能有错字、漏字、多字）、pinyin（输入法的切分，' 分隔，可能切错）、\
question（输入法本地把拼音转成的汉字，可能有错字，只是帮你理解问题；为空就自己还原）、max_items。

用户是**打不出某个字**才来问的：问题通常是问某个汉字——描述字形（san ge mu shi shen me zi → 森）、报部件（mu mu mu → 森）、\
描述读音或意思（biao shi gao xing de zi → 悦 / 欣 / 喜）；也可能是要一个很短的事实答案（fa guo shou du → 巴黎）。\
先把拼音还原成问题，再作答。**只给答案，绝不要把问题本身或它的汉字写法当作答案**：\
「三个直是什么字」答 矗，不答「三个直是什么字」；答案通常是一个字，几个可能的字各占一条。

输出 JSON：{\"answers\": [{\"text\": \"…\", \"pinyin\": \"…\"}]}

answers：1 到 max_items 个，按可能性排序。text 是能直接上屏的字、词或短答案，不要解释；\
pinyin 是 text 的带声调拼音（如 sēn），非中文答案给空字符串。不确定就少给，实在不懂就给空数组。";

/// 翻译的系统提示：中文选区译成学习语言，外文选区译回中文，只要译文。方向由 Core 按文字判断，模型兜底。
pub const TRANSLATE_SYSTEM_PROMPT: &str = "\
你是一个输入法的翻译助手。用户在应用里选中了一段文字并按了翻译快捷键，你会收到 JSON：\
text（选中的原文）、target_language（目标语言代码：zh 中文、en 英语、ja 日语）。\
把 text 完整、自然地译成目标语言，保留原文的语气、换行与标点习惯；\
原文已经是目标语言时：目标不是中文就改译成中文，目标是中文就原样返回。\
不要解释、不要加引号、不要加「译文：」之类的前缀。\
输出 JSON：{\"sentence\": \"译文\"}";

pub fn system_prompt(request: &PredictionRequest) -> &'static str {
    match request.kind {
        PredictionKind::Compose => SYSTEM_PROMPT,
        PredictionKind::Question => QUESTION_SYSTEM_PROMPT,
        PredictionKind::Translate => TRANSLATE_SYSTEM_PROMPT,
    }
}

/// 发给模型的用户消息：**只有用户敲的原始按键、光标前后文、候选窗第一页**，不做任何本地加工
///（不切分、不纠错、不塞本地整句），让模型自己理解意图。序列化出来就是模型看到的全部内容。
#[derive(Serialize)]
struct UserMessage<'a> {
    letters: &'a str,

    /// 用户当前的输入方式（「全拼」/「小鹤双拼」/「大千注音」/「五笔（86 版）」/「五笔（86 版）+ 全拼（混输）」…）。
    scheme: &'a str,

    /// 输出简体还是繁体。
    traditional: bool,

    before: &'a str,

    after: &'a str,

    candidates: &'a [String],

    max_items: usize,

    want_sentence: bool,
}

/// 问字模式发给模型的用户消息。
#[derive(Serialize)]
struct QuestionMessage<'a> {
    letters: &'a str,

    pinyin: &'a str,

    question: &'a str,

    max_items: usize,
}

/// 翻译发给模型的用户消息。
#[derive(Serialize)]
struct TranslateMessage<'a> {
    text: &'a str,

    target_language: &'a str,
}

pub fn user_prompt(request: &PredictionRequest) -> String {
    if request.kind == PredictionKind::Translate {
        return serde_json::to_string(&TranslateMessage {
            text: &request.text,
            target_language: &request.target_language,
        })
        .unwrap_or_default();
    }
    if request.kind == PredictionKind::Question {
        return serde_json::to_string(&QuestionMessage {
            letters: &request.letters,
            pinyin: &request.pinyin,
            question: &request.guess,
            max_items: request.max_items,
        })
        .unwrap_or_default();
    }
    serde_json::to_string(&UserMessage {
        letters: &request.letters,
        scheme: &request.scheme,
        traditional: request.traditional,
        before: &request.before,
        after: &request.after,
        candidates: &request.candidates,
        max_items: request.max_items,
        want_sentence: request.want_sentence,
    })
    .unwrap_or_default()
}

/// 解析后的回复。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reply {
    /// 云端词（组句联想）或答案（问字模式，音节为空、带读音）。
    pub words: Vec<CloudWord>,

    /// 整句补全。
    pub sentence: Option<String>,
}

impl Reply {
    /// 什么都没给：没有词也没有整句。
    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.sentence.is_none()
    }
}

/// 模型回复的原始形状，缺的字段当空。
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawReply {
    words: Vec<RawWord>,

    sentence: Option<String>,

    /// sentence 前 `syllables` 个字的全拼，用来和 letters 对。
    sentence_pinyin: Option<String>,

    /// 问字模式的答案。
    answers: Vec<RawWord>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawWord {
    text: String,

    pinyin: String,
}

/// 解析模型回复：去空、去重、去换行，截到 `max_items`。不要与本地首选相同的词，也不要没给拼音的词。
pub fn parse_reply(content: &str, request: &PredictionRequest) -> Reply {
    let raw: RawReply = match serde_json::from_str(content.trim()) {
        Ok(raw) => raw,
        Err(_) => return Reply::default(),
    };
    if request.kind == PredictionKind::Question {
        return parse_answers(raw.answers, request.max_items);
    }
    if request.kind == PredictionKind::Translate {
        // 译文保留换行（原文可能是多段），只去首尾空白
        return Reply {
            words: Vec::new(),
            sentence: raw
                .sentence
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty()),
        };
    }
    let mut reply = Reply::default();
    let mut seen: Vec<String> = Vec::new();
    for word in raw.words {
        let text = clean(&word.text);
        // `pinyin` 现在只是可选信息（提示词不再要求）：给了就带着（上屏时记用户词用得上），
        // 没给也不影响进候选 —— 本地不再拿它去校验。
        let syllables: Vec<String> = word
            .pinyin
            .split(|c: char| c.is_whitespace() || c == '\'')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_ascii_lowercase())
            .collect();
        // 不与候选窗第一页重复（用户定的口径：本地已经给了的东西不必再来一条），也不自己重复。
        if text.is_empty() || request.candidates.contains(&text) || seen.contains(&text) {
            continue;
        }
        seen.push(text.clone());
        reply.words.push(CloudWord {
            text,
            syllables,
            reading: None,
        });
        if reply.words.len() >= request.max_items {
            break;
        }
    }
    if request.want_sentence
        && let Some(raw_sentence) = raw.sentence
    {
        // 组装层面只做两件清理：剥掉模型重复写进来的 before / after（不剥的话上屏会重复一遍），
        // 以及整句与候选窗第一页完全一样时不必再来一条（本地已经给了）。
        // **不再拿拼音校验整句**：模型不用给 `sentence_pinyin`，拼音切不开的输入（夹英文词的）也能出整句。
        let sentence = strip_after(
            &strip_before(&clean(&raw_sentence), &request.before),
            &request.after,
        );
        let duplicated_local = request.after.is_empty() && request.candidates.contains(&sentence);
        let fits =
            !sentence.is_empty() && !duplicated_local && !restates_after(&sentence, &request.after);
        if fits {
            reply.sentence = Some(sentence);
        } else {
            tracing::debug!(
                %sentence,
                "整句补全不收（空 / 与候选窗第一页重复 / 复述下文）"
            );
        }
    }
    reply
}

/// 问字模式的答案：去空、去重，拼音只是显示用的读音，不校验。
fn parse_answers(answers: Vec<RawWord>, max_items: usize) -> Reply {
    let mut reply = Reply::default();
    for answer in answers {
        let text = clean(&answer.text);
        if text.is_empty() || reply.words.iter().any(|w| w.text == text) {
            continue;
        }
        let reading = clean(&answer.pinyin);
        reply.words.push(CloudWord {
            text,
            syllables: Vec::new(),
            reading: (!reading.is_empty()).then_some(reading),
        });
        if reply.words.len() >= max_items {
            break;
        }
    }
    reply
}

/// 模型爱把 before 也抄进整句里；整句只替换拼音，所以把与 before 尾部重叠的开头去掉。
fn strip_before(sentence: &str, before: &str) -> String {
    let before: Vec<char> = before.chars().collect();
    let chars: Vec<char> = sentence.chars().collect();
    // 从最长的重叠开始试：before 的后 k 个字符 == sentence 的前 k 个字符
    for k in (1..=before.len().min(chars.len())).rev() {
        if before[before.len() - k..] == chars[..k] {
            return chars[k..]
                .iter()
                .collect::<String>()
                .trim_start()
                .to_owned();
        }
    }
    sentence.to_owned()
}

/// 模型爱把 after（光标后面已有的文字）也抄进整句里；整句只替换拼音，抄进来会让那段文字重复一遍，
/// 所以把与 after 头部重叠的结尾去掉。与 [`strip_before`] 对称：从最长的重叠开始试。
fn strip_after(sentence: &str, after: &str) -> String {
    let after: Vec<char> = after.chars().collect();
    let chars: Vec<char> = sentence.chars().collect();
    for k in (1..=after.len().min(chars.len())).rev() {
        if chars[chars.len() - k..] == after[..k] {
            return chars[..chars.len() - k]
                .iter()
                .collect::<String>()
                .trim_end()
                .to_owned();
        }
    }
    sentence.to_owned()
}

/// 模型有时不逐字抄 after，而是改写一遍再接上来（sentence 大学阶段学习的重要基础课程之一、
/// after 最重要的基础课程之一——只差一个「最」和一个「的」），[`strip_after`] 剥不掉，接受之后文档里会重复一句。
/// 句子大半由 after 的字组成就算复述，整句不收：宁可不补，也不要拼出重复的一句。after 太短时不判（噪声太大）。
fn restates_after(sentence: &str, after: &str) -> bool {
    let after: Vec<char> = after.chars().collect();
    if after.len() < 4 {
        return false;
    }
    let sentence: Vec<char> = sentence.chars().collect();
    lcs_len(&sentence, &after) * 4 >= after.len() * 3
}

/// 最长公共子序列长度。两个串都很短（上文几十字），O(nm) 够用。
fn lcs_len(a: &[char], b: &[char]) -> usize {
    let mut prev = vec![0usize; b.len() + 1];
    let mut cur = vec![0usize; b.len() + 1];
    for &x in a {
        for (j, &y) in b.iter().enumerate() {
            cur[j + 1] = if x == y {
                prev[j] + 1
            } else {
                cur[j].max(prev[j + 1])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
        cur.fill(0);
    }
    prev[b.len()]
}

fn clean(text: &str) -> String {
    text.trim()
        .chars()
        .filter(|c| *c != '\n' && *c != '\r')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 整句里写阿拉伯数字、拼音按念法给（`123` → `yi bai er shi san`）：字母照样对得上，句子收得下。
    #[test]
    fn arabic_digits_pass_the_pinyin_check() {
        let reply = r#"{"words": [], "sentence": "我花123元", "sentence_pinyin": "wo hua yi bai er shi san yuan"}"#;
        // helper 的 syllables 默认写 2（给简拼测试用），这里按实际音节数写
        let mut req = request("wo'hua'yi'bai'er'shi'san'yuan", true);
        req.syllables = 8;
        let parsed = parse_reply(reply, &req);
        assert_eq!(parsed.sentence.as_deref(), Some("我花123元"));
    }

    /// 一句话里两种数字写法混着（用户的例子）：金额写阿拉伯数字、量词写汉字，
    /// 拼音按敲的字母给——`123` 按念法、`mp3` 按 `mp` 加 `san`。
    #[test]
    fn mixed_arabic_digits_and_letters_in_one_sentence() {
        let reply = r#"{"words": [], "sentence": "我花123元买了三个mp3",
            "sentence_pinyin": "wo hua yi bai er shi san yuan mai le san ge mp san"}"#;
        let mut req = request("wo'hua'yi'bai'er'shi'san'yuan'mai'le'san'ge'mp'san", true);
        req.syllables = 14;
        let parsed = parse_reply(reply, &req);
        assert_eq!(parsed.sentence.as_deref(), Some("我花123元买了三个mp3"));
    }

    fn request(pinyin: &str, want_sentence: bool) -> PredictionRequest {
        PredictionRequest {
            sequence: 1,
            kind: PredictionKind::Compose,
            before: "我们今天".into(),
            after: String::new(),
            pinyin: pinyin.into(),
            letters: pinyin.replace('\'', ""),
            scheme: "全拼".into(),
            traditional: false,
            syllables: 2,
            candidates: vec!["张涛".into(), "张贴".into()],
            guess: String::new(),
            max_items: 2,
            want_sentence,
            text: String::new(),
            target_language: String::new(),
        }
    }

    #[test]
    fn user_prompt_is_the_raw_input_and_first_page() {
        let prompt = user_prompt(&request("zhang'tao", true));
        // 只发用户原始输入 + 上下文 + 候选窗第一页；切分 / 音节数 / 本地整句都不再喂给模型
        assert!(prompt.contains("\"letters\":\"zhangtao\""));
        assert!(prompt.contains("\"before\":\"我们今天\""));
        assert!(prompt.contains("\"candidates\":[\"张涛\",\"张贴\"]"));
        assert!(prompt.contains("\"want_sentence\":true"));
        assert!(!prompt.contains("pinyin"));
        assert!(!prompt.contains("syllables"));
    }

    #[test]
    fn reply_keeps_words_and_drops_the_first_page_duplicates() {
        let reply = r#"{"words": [{"text": "账套", "pinyin": "zhang tao"}, {"text": "张涛", "pinyin": "zhang tao"},
            {"text": "涨停", "pinyin": ""}, {"text": " 章台 ", "pinyin": "Zhang'Tai"}, {"text": "张套", "pinyin": "zhang tao"}],
            "sentence": " 账套已经建好了\n"}"#;
        let parsed = parse_reply(reply, &request("zhang'tao", true));
        let texts: Vec<(&str, Vec<&str>)> = parsed
            .words
            .iter()
            .map(|w| {
                (
                    w.text.as_str(),
                    w.syllables.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        // 「张涛」与候选窗第一页重复 → 丢；「涨停」没给拼音也能进（本地不再校验拼音了）；
        // `max_items` 是 2，到这里就够数了。
        assert_eq!(texts, [("账套", vec!["zhang", "tao"]), ("涨停", vec![])]);
        assert_eq!(parsed.sentence.as_deref(), Some("账套已经建好了"));
        // 没要整句就不收
        assert_eq!(
            parse_reply(reply, &request("zhang'tao", false)).sentence,
            None
        );
        // 整句里抄了 before 的，去掉重叠部分
        let echoed = r#"{"words": [], "sentence": "我们今天账套已经建好了"}"#;
        assert_eq!(
            parse_reply(echoed, &request("zhang'tao", true))
                .sentence
                .as_deref(),
            Some("账套已经建好了")
        );
        let partial = r#"{"words": [], "sentence": "今天账套已经建好了"}"#;
        assert_eq!(
            parse_reply(partial, &request("zhang'tao", true))
                .sentence
                .as_deref(),
            Some("账套已经建好了")
        );
    }

    #[test]
    fn question_replies_keep_answers_with_readings() {
        let mut question = request("san'ge'mu", false);
        question.kind = PredictionKind::Question;
        question.candidates.clear();
        assert!(user_prompt(&question).contains("\"letters\":\"sangemu\""));
        assert!(!user_prompt(&question).contains("before"));
        let reply = r#"{"answers": [{"text": "森", "pinyin": "sēn"}, {"text": "森", "pinyin": "sēn"}, {"text": "巴黎", "pinyin": ""}]}"#;
        let parsed = parse_reply(reply, &question);
        assert_eq!(parsed.words.len(), 2);
        assert_eq!(parsed.words[0].text, "森");
        assert_eq!(parsed.words[0].reading.as_deref(), Some("sēn"));
        assert!(parsed.words[0].syllables.is_empty());
        assert_eq!(parsed.words[1].reading, None);
        assert_eq!(parsed.sentence, None);
    }

    #[test]
    fn reply_drops_the_after_text_that_the_model_echoed() {
        let mut request = request("liangg", true);
        request.before = "笛卡儿积是一种二元运算，把".into();
        request.after = "按顺序两两配对组成有序对。".into();
        // 模型把光标后面已有的文字抄了进来：剥掉，只留补拼音那段
        let echoed = r#"{"words": [], "sentence": "两个集合中的元素按顺序两两配对组成有序对。", "sentence_pinyin": "liang ge"}"#;
        assert_eq!(
            parse_reply(echoed, &request).sentence.as_deref(),
            Some("两个集合中的元素")
        );
        // before 的尾巴也被抄了进来：两头都剥
        let both = r#"{"words": [], "sentence": "把两个集合中的元素按顺序两两配对组成有序对。", "sentence_pinyin": "liang ge"}"#;
        assert_eq!(
            parse_reply(both, &request).sentence.as_deref(),
            Some("两个集合中的元素")
        );
        // 与下文无关时不误伤
        let unrelated =
            r#"{"words": [], "sentence": "两个集合做笛卡儿积。", "sentence_pinyin": "liang ge"}"#;
        assert_eq!(
            parse_reply(unrelated, &request).sentence.as_deref(),
            Some("两个集合做笛卡儿积。")
        );
        // 整句就是下文的原样：剥完为空，不收
        let only_after = r#"{"words": [], "sentence": "按顺序两两配对组成有序对。", "sentence_pinyin": "an shun"}"#;
        assert!(parse_reply(only_after, &request).sentence.is_none());
    }

    /// 模型不逐字抄 after，而是改写一遍再接上来（只差一个「最」和一个「的」）：逐字剥不掉，整句不收。
    #[test]
    fn reply_drops_a_sentence_that_rephrases_the_after_text() {
        let mut req = request("dx", true);
        req.before = "高等数学是".into();
        req.after = "最重要的基础课程之一".into();
        let rephrased = r#"{"words": [{"text": "大学", "pinyin": "da xue"}], "sentence": "大学阶段学习的重要基础课程之一", "sentence_pinyin": "da xue"}"#;
        let parsed = parse_reply(rephrased, &req);
        assert_eq!(parsed.words[0].text, "大学", "词那一路不受影响");
        assert!(
            parsed.sentence.is_none(),
            "复述了下文的整句不该收，实际 {:?}",
            parsed.sentence
        );
        // 真正填空的那几个字照常收
        let gap = r#"{"words": [], "sentence": "大学", "sentence_pinyin": "da xue"}"#;
        assert_eq!(parse_reply(gap, &req).sentence.as_deref(), Some("大学"));
        // 没有 after 时的正常完整句不受影响
        let mut bare = request("dx", true);
        bare.before = "高等数学是".into();
        let normal =
            r#"{"words": [], "sentence": "大学是人生的新起点。", "sentence_pinyin": "da xue"}"#;
        assert_eq!(
            parse_reply(normal, &bare).sentence.as_deref(),
            Some("大学是人生的新起点。")
        );
    }

    /// **不再校验整句的拼音**（2026-09-18 改口径）：模型不用给 `sentence_pinyin`，句子直接采纳 ——
    /// 这样「拼音切不开」的输入（夹着英文词的 `javashiyimenbianchengyuyan`）也能出整句。
    #[test]
    fn a_sentence_is_taken_as_is_without_any_pinyin() {
        let mut req = request("javashiyimenbianchengyuyan", true);
        req.before = String::new();
        req.candidates = vec!["就啊".into(), "骄傲".into()];
        let mixed = r#"{"words": [], "sentence": "Java 是一门编程语言"}"#;
        assert_eq!(
            parse_reply(mixed, &req).sentence.as_deref(),
            Some("Java 是一门编程语言")
        );
        // 模型按语义填了别的词（以前「拼音对不上」会被拒），现在照收
        let mut gap = request("dx", true);
        gap.before = "高等数学是一门非常重要的".into();
        gap.after = "课程".into();
        let semantic = r#"{"words": [], "sentence": "基础", "sentence_pinyin": "ji chu"}"#;
        assert_eq!(
            parse_reply(semantic, &gap).sentence.as_deref(),
            Some("基础")
        );
    }

    /// 续写（用户没敲拼音，敲了续写键按 Tab）：只看句子本身成不成立。
    #[test]
    fn continuation_replies_are_taken_as_is() {
        let mut req = request("", true);
        req.letters = String::new();
        req.candidates.clear();
        req.before = "笛卡儿积是一种二元运算，把两个集合".into();
        req.after = "按顺序两两配对组成有序对。".into();
        let reply = r#"{"words": [], "sentence": "中的元素"}"#;
        assert_eq!(
            parse_reply(reply, &req).sentence.as_deref(),
            Some("中的元素")
        );
        // 复述 after 照样不收（这条不因为续写就放宽）
        let echoed = r#"{"words": [], "sentence": "按顺序两两配对组成有序对。"}"#;
        assert!(parse_reply(echoed, &req).sentence.is_none());
        // 没给 sentence 就是空
        assert!(parse_reply(r#"{"words": []}"#, &req).sentence.is_none());
    }

    /// 有下文（填空）时，整句与候选窗第一页里某个候选相同也要收——它填的是 before 与 after 之间那一段，
    /// 与词候选用途不同，而答案常常就是本地那个词。
    #[test]
    fn a_gap_filling_sentence_may_match_a_first_page_candidate() {
        let mut req = request("da'xue", true);
        req.before = "高等数学是".into();
        req.after = "最重要的基础课程之一".into();
        req.candidates = vec!["大学".into(), "大雪".into()];
        let reply = r#"{"words": [], "sentence": "大学"}"#;
        assert_eq!(
            parse_reply(reply, &req).sentence.as_deref(),
            Some("大学"),
            "填空时答案就是本地那个词，不该当重复丢掉"
        );
        // 没有下文时仍然不收：本地已经给了同一个词，整句再来一条是重复
        let mut bare = request("da'xue", true);
        bare.candidates = vec!["大学".into()];
        assert!(parse_reply(reply, &bare).sentence.is_none());
    }

    #[test]
    fn garbage_replies_are_empty() {
        assert_eq!(
            parse_reply("not json", &request("k", false)),
            Reply::default()
        );
        assert_eq!(
            parse_reply(r#"{"foo": 1}"#, &request("zt", false)),
            Reply::default()
        );
    }

    #[test]
    fn translate_requests_use_their_own_prompt_and_keep_the_translation() {
        let mut translate = request("", false);
        translate.kind = PredictionKind::Translate;
        translate.text = "我想去吃饭".to_owned();
        translate.target_language = "en".to_owned();
        assert_eq!(system_prompt(&translate), TRANSLATE_SYSTEM_PROMPT);
        assert!(user_prompt(&translate).contains("\"target_language\":\"en\""));
        let reply = parse_reply(r#"{"sentence": "  I want to go eat.\n"}"#, &translate);
        assert_eq!(reply.sentence.as_deref(), Some("I want to go eat."));
        assert!(reply.words.is_empty());
        assert!(
            parse_reply(r#"{"sentence": ""}"#, &translate)
                .sentence
                .is_none()
        );
    }
}
