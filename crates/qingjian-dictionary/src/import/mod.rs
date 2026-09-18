//! 导入词库：把用户给的文件（青简 TSV、Rime `.dict.yaml`、现成的 `.qj`）变成用户词库目录里的一个 `.qj`。
//!
//! 导入时只做格式转换，不做拼音校验：不合法的音节查不到而已，不影响别的词。
//! 目标文件名取源文件的主干（`law.dict.yaml` → `law.qj`），已存在就覆盖（重新导入即更新）。

mod imported;
mod rime;

use std::path::{Path, PathBuf};

pub use imported::Imported;
use qingjian_format::{Container, Metadata};

use crate::dictionary::Dictionary;
use crate::error::DictionaryError;

/// 把 `source` 导入到 `dest_dir`，返回写出的文件与元数据。
pub fn import(source: &Path, dest_dir: &Path) -> Result<Imported, DictionaryError> {
    let stem = stem_of(source)?;
    std::fs::create_dir_all(dest_dir)?;
    let target = dest_dir.join(format!("{stem}.qj"));
    let (dictionary, metadata) = read(source)?;
    if dictionary.is_empty() {
        return Err(DictionaryError::Corrupt(
            "no usable entries; expected word and explicit pinyin columns",
        ));
    }
    dictionary.write_qj(&target, &metadata)?;
    Ok(Imported {
        path: target,
        name: metadata.name,
        entries: dictionary.len(),
    })
}

/// 从任意支持的文件读成一份词库：`.qj` 走容器，其余按文本解析（Rime YAML 头先转 TSV）。
///
/// **导入与「直接把文件放进词库目录」两条路共用它** —— 否则两边认的格式迟早分叉：2026-09-18
/// 就是导入对话框列了 `.dict.yaml`、而目录扫描只认 `.qj`/`.tsv`，用户把 YAML 放进目录等于没放。
pub fn read(source: &Path) -> Result<(Dictionary, Metadata), DictionaryError> {
    let stem = stem_of(source)?;
    if Container::is_qj(source) {
        let dictionary = Dictionary::open_qj(source)?;
        let metadata = dictionary.metadata().cloned().unwrap_or_default();
        return Ok((dictionary, metadata));
    }
    let text = std::fs::read_to_string(source)?;
    let (tsv, name) = if rime::looks_like_rime(&text) {
        let parsed = rime::to_tsv(&text);
        (parsed.tsv, parsed.name)
    } else {
        (text, None)
    };
    let dictionary = Dictionary::parse(&tsv)?;
    let metadata = Metadata {
        name: name.unwrap_or_else(|| stem.clone()),
        source: source.display().to_string(),
        ..Metadata::default()
    };
    Ok((dictionary, metadata))
}

/// 词库名（去掉已知后缀）：`law.dict.yaml` → `law`、`dict.tsv` → `dict`。
/// 目录扫描（`qingjian_platform::extra_dictionaries`）也用它，两边对同名的判断才一致。
pub fn stem(file_name: &str) -> String {
    strip_extensions(file_name)
}

/// 从路径取词库名；取不出（没有文件名 / 去完后缀为空）就报错。
fn stem_of(source: &Path) -> Result<String, DictionaryError> {
    source
        .file_name()
        .and_then(|n| n.to_str())
        .map(strip_extensions)
        .filter(|s| !s.is_empty())
        .ok_or(DictionaryError::Corrupt("source has no usable file name"))
}

/// `law.dict.yaml` → `law`，`dict.tsv` → `dict`。
fn strip_extensions(file_name: &str) -> String {
    let mut stem = file_name;
    for suffix in [".dict.yaml", ".yaml", ".yml", ".tsv", ".txt", ".qj"] {
        if let Some(s) = stem.strip_suffix(suffix) {
            stem = s;
            break;
        }
    }
    stem.to_owned()
}

/// 词库目录里的 `.qj` 文件名。
pub fn target_name(stem: &str) -> PathBuf {
    PathBuf::from(format!("{stem}.qj"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_tsv_and_rime_into_qj() {
        let dir = std::env::temp_dir().join(format!("qingjian-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tsv = dir.join("finance.tsv");
        std::fs::write(&tsv, "账套\tzhang tao\t500\n").unwrap();
        let imported = import(&tsv, &dir.join("dicts")).unwrap();
        assert_eq!(imported.name, "finance");
        assert_eq!(imported.entries, 1);
        let dictionary = Dictionary::from_path(&imported.path).unwrap();
        assert_eq!(dictionary.lookup(&["zhang", "tao"], false)[0].text, "账套");
        assert_eq!(dictionary.metadata().unwrap().name, "finance");

        let rime = dir.join("law.dict.yaml");
        std::fs::write(
            &rime,
            "# Rime dictionary\n---\nname: law\nversion: \"1\"\nsort: by_weight\n...\n合同法\the tong fa\t120\n民法典\tmin fa dian\n",
        )
        .unwrap();
        let imported = import(&rime, &dir.join("dicts")).unwrap();
        assert_eq!(imported.path.file_name().unwrap(), "law.qj");
        assert_eq!(imported.name, "law");
        assert_eq!(imported.entries, 2);
        let dictionary = Dictionary::from_path(&imported.path).unwrap();
        assert_eq!(
            dictionary.lookup(&["min", "fa", "dian"], false)[0].text,
            "民法典"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn strips_known_extensions() {
        assert_eq!(strip_extensions("law.dict.yaml"), "law");
        assert_eq!(strip_extensions("dict.tsv"), "dict");
        assert_eq!(strip_extensions("x.qj"), "x");
        assert_eq!(strip_extensions("noext"), "noext");
    }
}
