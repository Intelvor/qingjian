//! 附加词库：随包的领域词库（`Resources/dicts/`，按 `[dictionaries] domains` 挑）与用户目录 `dicts/` 下的 `.qj`（或 TSV）
//! 文件（按 `[dictionaries] disabled` 过滤），加载后一起接到 Engine 上。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::DictionariesConfig;
use qingjian_dictionary::Dictionary;

/// 目录里能加载的扩展名，靠前的优先：同名的 `.qj` 与 `.tsv` 只取 `.qj`（开发目录里两者并存）。
///
/// **与「导入词库」认的格式保持一致** —— 放进去和导入进来必须是同一批格式。2026-09-18 的 bug
/// 就是这里只写 `qj`/`tsv`，而导入对话框列了 `.dict.yaml`，用户把 YAML 放进目录被静默忽略。
const EXTENSIONS: [&str; 5] = ["qj", "tsv", "yaml", "yml", "txt"];

/// 可加载文件的快照：用于发现新增、移除与同名更新，不读取词库正文。
pub fn snapshot(dir: &Path) -> Vec<(PathBuf, Option<SystemTime>, u64)> {
    list(dir)
        .into_iter()
        .filter_map(|(_, path)| {
            let metadata = std::fs::metadata(&path).ok()?;
            Some((path, metadata.modified().ok(), metadata.len()))
        })
        .collect()
}

/// 隐藏文件 / macOS 的 AppleDouble 元数据（`._animals.qj`，163 字节）不当词库看。
///
/// 产品数据的 tar 包里常跟着一批 `._*`（macOS 打包时给每个文件配的资源叉），它们**后缀正好是 `.qj`**，
/// 不排掉就会出现在设置页的词库列表里、显示成「读不了：文件损坏或格式不对」（2026-09-18 用户截图反馈）。
pub fn is_metadata_file(name: &str) -> bool {
    name.starts_with('.')
}

/// 列出目录里的词库文件（按文件名排序，同名只留优先扩展名的那个），返回 (文件名不含扩展名, 路径)。目录不存在就是空。
pub fn list(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(String, usize, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?;
            if is_metadata_file(name) {
                return None;
            }
            let extension = p.extension()?.to_str()?;
            let rank = EXTENSIONS.iter().position(|e| *e == extension)?;
            // 用导入那边同一套去后缀：`law.dict.yaml` 与 `law.qj` 要算同一本词库。
            let stem = qingjian_dictionary::import::stem(name);
            (!stem.is_empty()).then_some((stem, rank, p))
        })
        .collect();
    files.sort();
    files.dedup_by(|a, b| a.0 == b.0);
    files
        .into_iter()
        .map(|(stem, _, path)| (stem, path))
        .collect()
}

/// 加载没被关掉的词库：先随包领域词库，再用户目录。坏文件只记日志、跳过：一本词库坏了不能拖垮输入法。
pub fn load(
    bundled_dir: Option<&Path>,
    user_dir: Option<&Path>,
    config: &DictionariesConfig,
) -> Vec<Dictionary> {
    let mut loaded = Vec::new();
    if let Some(dir) = bundled_dir {
        for (stem, path) in list(dir) {
            if !config.is_domain_enabled(&stem) {
                tracing::debug!(name = %stem, "随包领域词库未打开，跳过");
                continue;
            }
            loaded.extend(open(&stem, &path));
        }
    }
    if let Some(dir) = user_dir {
        for (stem, path) in list(dir) {
            if !config.is_enabled(&stem) {
                tracing::debug!(name = %stem, "附加词库已关闭，跳过");
                continue;
            }
            loaded.extend(open(&stem, &path));
        }
    }
    loaded
}

fn open(stem: &str, path: &Path) -> Option<Dictionary> {
    // 走导入那条同一套读取：`.qj` 容器 / TSV / Rime `.dict.yaml` / `.txt` 都认。
    match qingjian_dictionary::import::read(path) {
        Ok((dictionary, metadata)) => {
            tracing::info!(
                name = %metadata.name,
                file = %path.display(),
                entries = dictionary.len(),
                license = %metadata.license,
                "附加词库已加载"
            );
            Some(dictionary)
        }
        Err(error) => {
            tracing::warn!(file = %path.display(), stem = %stem, %error, "附加词库加载失败，跳过");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 目录认的后缀与「导入词库」必须是同一批：`.qj` / `.tsv` / Rime `.dict.yaml` / `.txt` 都算，
    /// 不认识的（`.md`）不进列表；`.dict.yaml` 与 `.qj` 算同一个名字（同名取 `.qj`）。
    /// 隐藏 / 元数据文件（`._animals.qj`）一律不进列表。
    #[test]
    fn list_takes_the_same_formats_as_import() {
        let dir = std::env::temp_dir().join(format!("qingjian-extra-dicts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in [
            "idioms.qj",
            "idioms.tsv",
            "food.tsv",
            "notes.txt",
            "vibe.dict.yaml",
            "readme.md",
            // macOS 的 AppleDouble：后缀也是 `.qj`，但这不是词库
            "._idioms.qj",
            "._food.qj",
            ".hidden.qj",
        ] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        // 目录（`removed/` 这种）不该被当成词库文件列出来。
        std::fs::create_dir_all(dir.join("removed")).unwrap();

        let listed = list(&dir);
        let names: Vec<(&str, &str)> = listed
            .iter()
            .map(|(stem, path)| (stem.as_str(), path.file_name().unwrap().to_str().unwrap()))
            .collect();
        assert_eq!(
            names,
            [
                ("food", "food.tsv"),
                ("idioms", "idioms.qj"),
                ("notes", "notes.txt"),
                ("vibe", "vibe.dict.yaml"),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 用户把 Rime `.dict.yaml` 直接放进词库目录时要真的加载进来（2026-09-18 的 bug：
    /// 目录扫描只认 `qj`/`tsv`，放进去等于没放，而且静默无提示）。
    #[test]
    fn loads_a_rime_yaml_dropped_into_the_user_dir() {
        let dir = std::env::temp_dir().join(format!("qingjian-rime-dict-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("law.dict.yaml"),
            "---\nname: law\nversion: \"1\"\nsort: by_weight\n...\n合同法\the tong fa\t120\n",
        )
        .unwrap();

        let config = DictionariesConfig {
            disabled: Vec::new(),
            ..Default::default()
        };
        let loaded = load(None, Some(&dir), &config);
        assert_eq!(loaded.len(), 1, "放进去的 YAML 要被加载");
        // 文本格式（TSV / Rime）是现读现解析的，元数据由调用方另拿（只有 `.qj` 自带一份），
        // 所以这里只断言词条真的进来了、查得到。
        assert_eq!(
            loaded[0].lookup(&["he", "tong", "fa"], false)[0].text,
            "合同法"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
