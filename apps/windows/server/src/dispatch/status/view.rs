use qingjian_platform::{Scheme, ThemeMode};

/// 状态条一次要显示的内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusView {
    /// 英文模式（`false` 中文）。
    pub english: bool,

    /// 拼音侧方案（`[general] scheme`）：模式格按它出「拼 / 双 / 注」。
    pub scheme: Scheme,

    /// 形码侧开着（`[general] wubi`）：模式格后面再跟一个「五」（两条轴可以同时开着 = 混输）。
    pub wubi: bool,

    /// Caps Lock 亮着（大写锁定）：模式格前面加一个「A」。
    pub caps: bool,

    /// 当前模式的全角标点开着（中英各记一份配置）；关着时格子显示 `,.` 画成灰的。
    pub full_width: bool,

    /// 在线联想开着（`[predict] enabled`）：云朵格开着画品牌色、关着画灰的。
    pub cloud: bool,

    /// 外观模式。
    pub theme: ThemeMode,

    /// 配置里记住的内容左上角物理像素；`None` 首次按屏幕右下角摆。
    pub anchor: Option<(i32, i32)>,
}

impl StatusView {
    /// 模式格的文字：Caps Lock 亮着时**只出「A」**（这时无论中英模式都直接出大写英文，跟汉字挤一格没必要，
    /// 与任务栏那个图标一个口径）；否则是拼音侧一个字（全拼 `拼` / 双拼 `双` / 大千注音 `注`）加上形码侧的
    /// `五`（开着才加），英文模式出 `英`。
    ///
    /// **写一个字、不写方案全名**（2026-09-18 定）：`中 · 小浪双拼` 这类写法会随配置变长变短、把整条撑宽，
    /// 而它只在换方案时才变；方案全名去设置页「通用」看。
    pub fn mode_text(&self) -> String {
        if self.caps {
            return "A".to_owned();
        }
        if self.english {
            return "英".to_owned();
        }
        self.scheme_letters()
    }

    /// 中文模式（Caps 没亮）下的那串字：拼音侧一个字 + 形码的 `五`。英文模式与 Caps 都是把它整个换掉。
    fn scheme_letters(&self) -> String {
        let mut text = match self.scheme {
            Scheme::Pinyin => "拼".to_owned(),
            Scheme::Shuangpin(_) => "双".to_owned(),
            Scheme::Zhuyin => "注".to_owned(),
            // 拼音侧关着（只用形码）：这一格就只剩「五」。
            Scheme::Off => String::new(),
        };
        if self.wubi {
            text.push('五');
        }
        if text.is_empty() {
            // 两条轴都关着（配置写成了 scheme = "none" 又没开五笔）：一个候选都出不来，
            // 但也别显示空格子，退回以前的「中」。
            text.push('中');
        }
        text
    }

    /// 模式格的**宽度基准**：判据是「同一份配置下来回切时整条长度不变」，所以按这份配置里**最宽**的写法量 ——
    /// 中文模式那一串（`拼五` 这类）。Caps 的 `A` 与英文的 `英` 都比它窄，居中画在这个槽里。
    pub fn mode_width(&self) -> String {
        self.scheme_letters()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qingjian_core::ShuangpinScheme;

    fn view(scheme: Scheme, wubi: bool) -> StatusView {
        StatusView {
            english: false,
            scheme,
            wubi,
            caps: false,
            full_width: true,
            cloud: false,
            theme: ThemeMode::default(),
            anchor: None,
        }
    }

    /// 拼音侧一个字：全拼 `拼`、双拼 `双`、注音 `注`；形码开着在后面加 `五`；英文模式只出 `英`。
    #[test]
    fn mode_text_is_one_letter_per_axis() {
        assert_eq!(view(Scheme::Pinyin, false).mode_text(), "拼");
        assert_eq!(
            view(Scheme::Shuangpin(ShuangpinScheme::Xiaohe), false).mode_text(),
            "双"
        );
        assert_eq!(view(Scheme::Zhuyin, false).mode_text(), "注");
        assert_eq!(view(Scheme::Pinyin, true).mode_text(), "拼五");
        assert_eq!(
            view(Scheme::Shuangpin(ShuangpinScheme::Microsoft), true).mode_text(),
            "双五"
        );
        assert_eq!(view(Scheme::Zhuyin, true).mode_text(), "注五");
        // 只用形码：拼音侧关着，只剩「五」
        assert_eq!(view(Scheme::Off, true).mode_text(), "五");
        // 两条轴都关：什么也打不出来，别给空格子
        assert_eq!(view(Scheme::Off, false).mode_text(), "中");
    }

    /// 英文模式出「英」：五笔在英文模式下不参与，不跟着显示。
    #[test]
    fn english_mode_shows_only_ying() {
        let mut v = view(Scheme::Pinyin, true);
        v.english = true;
        assert_eq!(v.mode_text(), "英");
    }

    /// Caps 亮着时只出「A」：不跟汉字挤一格。
    #[test]
    fn caps_lock_replaces_the_mode_cell() {
        let mut v = view(Scheme::Shuangpin(ShuangpinScheme::Ziranma), true);
        v.caps = true;
        assert_eq!(v.mode_text(), "A");
        v.english = true;
        assert_eq!(v.mode_text(), "A");
    }

    /// 宽度基准是这份配置里最宽的写法（中文模式那串）：同一配置下来回切（中英、Caps 亮灭）不会改整条长度。
    #[test]
    fn width_reference_covers_every_variation_of_the_same_config() {
        assert_eq!(view(Scheme::Pinyin, false).mode_width(), "拼");
        assert_eq!(
            view(Scheme::Shuangpin(ShuangpinScheme::Sogou), true).mode_width(),
            "双五"
        );
        assert_eq!(view(Scheme::Zhuyin, true).mode_width(), "注五");
        assert_eq!(view(Scheme::Off, true).mode_width(), "五");
        for (scheme, wubi) in [
            (Scheme::Pinyin, false),
            (Scheme::Pinyin, true),
            (Scheme::Shuangpin(ShuangpinScheme::Xiaolang), false),
            (Scheme::Shuangpin(ShuangpinScheme::Xiaolang), true),
            (Scheme::Zhuyin, true),
            (Scheme::Off, true),
        ] {
            let width = view(scheme, wubi).mode_width();
            for english in [false, true] {
                for caps in [false, true] {
                    let mut v = view(scheme, wubi);
                    v.english = english;
                    v.caps = caps;
                    assert!(
                        v.mode_text().chars().count() <= width.chars().count(),
                        "{:?} wubi={wubi} english={english} caps={caps} 的文字比宽度基准还长：{} > {width}",
                        scheme,
                        v.mode_text()
                    );
                }
            }
        }
    }
}
