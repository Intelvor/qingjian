//! 一帧要画的全部内容：顶部拼音行、候选行、高亮、页脚、右侧整句补全。只是展示形态，不含排序或查词。

mod preedit;
mod row;
mod tone;

pub use preedit::{Preedit, PreeditSegment, PreeditStyle};
pub use row::Row;
pub use tone::Tone;

/// 鼠标停在哪个可点的东西上：给比键盘高亮淡一档的底色，让壳的悬停与点击看得见反馈。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hover {
    /// 第 `row` 行候选（帧内序号，与 [`Frame::highlighted`] 同一套下标）。
    Row(usize),

    /// 拼音行右侧那条整句补全。
    Trailing,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frame {
    /// 顶部拼音行；配置成只在行内显示时为 `None`。
    pub preedit: Option<Preedit>,

    /// 候选行。
    pub rows: Vec<Row>,

    /// 高亮行；`None` 不高亮。
    pub highlighted: Option<usize>,

    /// 鼠标悬停的地方；`None` 没有。与 [`highlighted`](Self::highlighted) 撞上时以高亮为准。
    pub hovered: Option<Hover>,

    /// 整句请求发出去了、结果还没到：那一块先摆 `☁ …`，结果到了原地换成整句。
    /// 只占位，不可点（没有内容可点）。
    pub sentence_pending: bool,

    /// 右下角页码。
    pub footer: Option<String>,

    /// 拼音行右侧的整句补全（云联想），组句时才有。
    pub sentence: Option<String>,

    /// 拼音行右侧的一句临时状态（删了什么词），有它时不画整句补全。
    pub status: Option<String>,
}

/// 拼音行右侧那一段。
#[derive(Debug, Clone, Copy)]
pub struct Trailing<'a> {
    /// 要画的文字。
    pub text: &'a str,

    /// 前面带不带云朵。
    pub cloud: bool,

    /// 是不是能点的真整句：等待占位那几个点没有内容可点，画在同一块位置上但不给命中小。
    pub pointable: bool,
}

/// 整句请求在路上时占位的几个点（真结果到了换成句子）。
const PENDING_DOTS: &str = "…";

impl Frame {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.preedit.is_none() && self.trailing().is_none()
    }

    /// 顶部要不要画一行（拼音或右侧文字任一存在）。
    pub fn has_top_line(&self) -> bool {
        self.preedit.is_some() || self.trailing().is_some()
    }

    /// 拼音行右侧画什么：临时状态优先，其次整句补全，再其次等待占位的那几个点。
    pub fn trailing(&self) -> Option<Trailing<'_>> {
        if let Some(status) = self.status.as_deref() {
            return Some(Trailing {
                text: status,
                cloud: false,
                pointable: false,
            });
        }
        if let Some(sentence) = self.sentence.as_deref() {
            return Some(Trailing {
                text: sentence,
                cloud: true,
                pointable: true,
            });
        }
        self.sentence_pending.then_some(Trailing {
            text: PENDING_DOTS,
            cloud: true,
            pointable: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 整句请求在路上时，那一块摆占位的点；有真句子时不摆，占位也不可点。
    #[test]
    fn pending_placeholder_shows_dots_but_is_not_pointable() {
        assert!(Frame::default().trailing().is_none(), "什么都没有时不占位");

        let waiting = Frame {
            sentence_pending: true,
            ..Frame::default()
        };
        let tail = waiting.trailing().expect("等待时该有占位");
        assert!(tail.cloud, "占位也带云朵，位置与结果一致");
        assert!(!tail.pointable, "占位没有内容可点");

        let arrived = Frame {
            sentence_pending: false,
            sentence: Some("你好".to_owned()),
            ..Frame::default()
        };
        let tail = arrived.trailing().expect("结果到了该显示句子");
        assert_eq!(tail.text, "你好");
        assert!(tail.pointable);
    }
}
