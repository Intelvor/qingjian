//! 前缀模式键：表达式 / 问字模式的入口字母，以及 `?` 是否也当问字入口。

use serde::{Deserialize, Serialize};

use crate::shortcut::EXPRESSION_PREFIX;

/// 问字模式的标点入口：[`ModeKeys::question_mark`] 打开时任何配置下 `?` 开头都进问字模式，也是英文模式下唯一的入口。
pub const QUESTION_PREFIX: char = '?';

/// 前缀模式键，配置文件 `[shortcut]` 分节。
///
/// 搜狗 / 微软那一家的做法：用不能开头拼任何音节的字母（`v` `u` `i`）一键进模式，
/// 不要修饰键，中文模式下零冲突。缺省 `v` 表达式、`u` 问字（含 Unicode 码点）、`i` 续写。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModeKeys {
    /// 表达式模式前缀：`v1+2`、`v123`。
    pub expression: char,

    /// 问字模式前缀：`usangemu`（三个木）、`u4e00`（码点）。
    pub question: char,

    /// 续写模式前缀：敲它之后按 Tab 请云端按光标前后文续写，前缀本身不进拼音。
    #[serde(rename = "continue")]
    pub continue_key: char,

    /// `?` 开头是否也进问字模式。缺省关：没在组句时敲的问号就是问号；
    /// 开着时缓冲区为空敲 `?` 先进问字，后面跟字母才是问题，跟别的键还原成问号。
    pub question_mark: bool,
}

impl Default for ModeKeys {
    fn default() -> Self {
        Self {
            expression: EXPRESSION_PREFIX,
            question: 'u',
            continue_key: 'i',
            question_mark: false,
        }
    }
}

impl ModeKeys {
    /// 能当模式键的字母：不是任何拼音音节的开头。
    pub const CANDIDATES: [char; 3] = ['v', 'u', 'i'];

    /// 去掉字母模式键：双拼下 v / u / i 都是音节键，只剩 `?`（开着的话）进问字。
    pub fn letterless(self) -> Self {
        Self {
            expression: '\0',
            question: '\0',
            continue_key: '\0',
            question_mark: self.question_mark,
        }
    }

    /// 三个键都合法且互不相同。不合法的配置整个退回缺省，不做一半。
    pub fn is_valid(&self) -> bool {
        let keys = [self.expression, self.question, self.continue_key];
        keys.iter().all(|key| Self::CANDIDATES.contains(key))
            && keys[0] != keys[1]
            && keys[0] != keys[2]
            && keys[1] != keys[2]
    }

    /// 非法配置退回缺省（`?` 开关照旧保留）；只有续写键撞车时，把它挪到没被占用的那个候选键。
    pub fn sanitized(self) -> Self {
        if self.is_valid() {
            return self;
        }
        // 表达式 / 问字本身不合法（相同，或用了不能当模式键的字母）：整个退回缺省，不做一半。
        if self.expression == self.question
            || !Self::CANDIDATES.contains(&self.expression)
            || !Self::CANDIDATES.contains(&self.question)
        {
            return Self {
                question_mark: self.question_mark,
                ..Self::default()
            };
        }
        // 只是续写键被前两个占了（旧配置常把 `i` 配给表达式 / 问字）：挪到剩下的那个键。
        let free = Self::CANDIDATES
            .into_iter()
            .find(|key| *key != self.expression && *key != self.question)
            .unwrap_or(self.continue_key);
        Self {
            continue_key: free,
            ..self
        }
    }

    pub fn is_expression(&self, input: &str, zhuyin: bool) -> bool {
        input.starts_with(self.expression)
            && (!zhuyin || crate::zhuyin::layout::map_key(self.expression).is_none())
    }

    pub fn is_question(&self, input: &str, zhuyin: bool) -> bool {
        (input.starts_with(self.question)
            && (!zhuyin || crate::zhuyin::layout::map_key(self.question).is_none()))
            || self.is_question_mark(input)
    }

    /// 是否以续写键开头：敲它之后按 Tab 请云端按光标前后文续写，前缀不进拼音。
    /// 跟表达式 / 问字一样，双拼与注音下那几个字母都是音节键，不让位。
    pub fn is_continue(&self, input: &str, zhuyin: bool) -> bool {
        input.starts_with(self.continue_key)
            && (!zhuyin || crate::zhuyin::layout::map_key(self.continue_key).is_none())
    }

    /// 是否以 `?` 进的问字模式：开关关着时 `?` 不是入口。
    fn is_question_mark(&self, input: &str) -> bool {
        self.question_mark && input.starts_with(QUESTION_PREFIX)
    }

    /// 问字模式下前缀之后的部分。不在问字模式时原样返回。
    pub fn question_body<'a>(&self, input: &'a str, zhuyin: bool) -> &'a str {
        if self.is_question_mark(input) {
            &input[QUESTION_PREFIX.len_utf8()..]
        } else if self.is_question(input, zhuyin) {
            &input[self.question.len_utf8()..]
        } else {
            input
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_v_and_u_and_question_mark_is_off() {
        let keys = ModeKeys::default();
        assert!(keys.is_expression("v12", false));
        assert!(keys.is_question("usangemu", false));
        assert!(keys.is_continue("i", false));
        assert!(!keys.is_continue("nihao", false));
        assert!(!keys.is_question("?sangemu", false));
        assert_eq!(keys.question_body("usangemu", false), "sangemu");
        assert_eq!(keys.question_body("?sangemu", false), "?sangemu");
        assert!(!keys.is_question("nihao", false));
    }

    #[test]
    fn question_mark_is_an_alias_only_when_switched_on() {
        let keys = ModeKeys {
            question_mark: true,
            ..ModeKeys::default()
        };
        assert!(keys.is_question("?sangemu", false));
        assert_eq!(keys.question_body("?sangemu", false), "sangemu");
        // 双拼下字母键让位，只剩 `?`；开关跟着走
        let letterless = keys.letterless();
        assert!(!letterless.is_question("usangemu", false));
        assert!(letterless.is_question("?sangemu", false));
        assert!(!ModeKeys::default().letterless().is_question("?x", false));
    }

    #[test]
    fn invalid_combinations_fall_back_to_defaults() {
        let same = ModeKeys {
            expression: 'v',
            question: 'v',
            continue_key: 'i',
            question_mark: true,
        };
        assert!(!same.is_valid());
        assert_eq!(
            same.sanitized(),
            ModeKeys {
                question_mark: true,
                ..ModeKeys::default()
            }
        );
        let pinyin_initial = ModeKeys {
            expression: 'v',
            question: 'z',
            continue_key: 'i',
            question_mark: false,
        };
        assert_eq!(pinyin_initial.sanitized(), ModeKeys::default());
        let swapped = ModeKeys {
            expression: 'i',
            question: 'v',
            continue_key: 'u',
            question_mark: false,
        };
        assert!(swapped.is_valid());
        assert!(swapped.is_question("v4e00", false));
        // 旧配置把 `i` 配给了表达式 / 问字、又没写 continue：只挪续写键，不整个退回缺省
        let taken = ModeKeys {
            expression: 'i',
            question: 'u',
            continue_key: 'i',
            question_mark: false,
        };
        assert!(!taken.is_valid());
        let fixed = taken.sanitized();
        assert_eq!(fixed.expression, 'i');
        assert_eq!(fixed.question, 'u');
        assert_eq!(fixed.continue_key, 'v');
    }

    #[test]
    fn deserializes_from_single_character_strings() {
        let keys: ModeKeys = toml::from_str(
            "expression = \"i\"\nquestion = \"u\"\ncontinue = \"v\"\nquestion_mark = true\n",
        )
        .unwrap();
        assert_eq!(keys.expression, 'i');
        assert_eq!(keys.question, 'u');
        assert_eq!(keys.continue_key, 'v');
        assert!(keys.question_mark);
        // 旧文件没有 `continue`：取缺省 `i`
        let old: ModeKeys = toml::from_str("expression = \"v\"\nquestion = \"u\"\n").unwrap();
        assert!(!old.question_mark);
        assert_eq!(old.continue_key, 'i');
    }
}
