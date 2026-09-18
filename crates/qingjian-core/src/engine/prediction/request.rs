/// 一次联想请求：要这段拼音对应的词，可能还要整句补全。只在组句中发，不在上屏后发。
///
/// 上下文只来自应用给出的光标前后文本；应用给不出就只靠拼音。本地输入历史是跨应用拼起来的碎片，不当上下文用。
use super::kind::PredictionKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PredictionRequest {
    /// 请求序号，单调递增；回来的结果序号对不上就是过期结果。
    pub sequence: u64,

    /// 组句联想还是问字。
    pub kind: PredictionKind,

    /// 应用里光标前的文本，已按观察窗口裁剪；应用不给时为空。
    pub before: String,

    /// 应用里光标后的文本，已按观察窗口裁剪。
    pub after: String,

    /// 正在输入的拼音（带切分 `'`），本地切分的结果，拼音有误时切分也可能是错的。
    pub pinyin: String,

    /// 用户实际敲的字母（不含 `'`），给模型纠错用。**区分大小写**：全拼 / 五笔走缓冲区原样
    /// （`shift_letter = "compose"` 时 Shift 敲的大写会还原进这里，模型可据此判 `GitHub` 这类专有名词）；
    /// 双拼 / 注音发的是解出来的全拼（键位本身不带内容大小写）。
    pub letters: String,

    /// 用户当前的**输入方式**（中文标签，给模型看）：「全拼」/「小鹤双拼」/「大千注音」/「五笔（86 版）」/
    /// 「五笔 + 全拼」这种。**五笔（或混输）下 `letters` 是字根编码、不是拼音**（`wqiy` 是「你」），
    /// 模型得知道这回事才不会当拼音猜。
    pub scheme: String,

    /// 输出简体还是繁体（`[general] traditional`）：让模型直接用对应字形写，
    /// 免得它按简体写完再被本地转一遍。
    pub traditional: bool,

    /// 本地切分的音节数，只是参考。
    pub syllables: usize,

    /// 本地词库给出的前几个候选，帮助模型理解拼音，也让它别重复第一个。
    pub candidates: Vec<String>,

    /// 本地整句转换把这段拼音转成的汉字（问字模式是问题本身的汉字形式），可能有错字；转不出为空。
    pub guess: String,

    /// 最多要几条。
    pub max_items: usize,

    /// 是否还要整句补全。
    pub want_sentence: bool,

    /// 翻译请求的原文（应用里选中的文字）；其他种类为空。
    pub text: String,

    /// 翻译请求的目标语言代码（`en` / `ja`，学习语言）；其他种类为空。
    pub target_language: String,
}
