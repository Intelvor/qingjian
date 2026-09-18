//! 「词库」页：随包领域词库开关，用户导入词库的开关 / 移除 / 导入。
//! 随包开关写 `[dictionaries] domains`（列打开的），用户词库写 `disabled`（列关掉的）。

use std::path::{Path, PathBuf};

use qingjian_platform::extra_dictionaries;
use windows_reactor::*;

use crate::panel::controls::{note, page, repo_resource};
use crate::panel::{Message, Settings};

/// 用户词库目录 `%APPDATA%\Qingjian\dicts`。
fn user_dir(settings: &Settings) -> PathBuf {
    settings.data_dir().join("dicts")
}

/// 一行词库的显示文字 + 是不是读不了（读不了的复选框要禁用）。
///
/// 走的是**导入那边同一条读取**（`.qj` / TSV / Rime `.dict.yaml` / `.txt`），所以这里认的
/// 和「放进目录能被加载的」永远是同一批格式；读不出来也**说清原因**，不再一律「文件损坏」。
fn row_label(path: &Path, stem: &str, builtin: bool) -> (String, bool) {
    match qingjian_core::dictionary::import::read(path) {
        Ok((dictionary, metadata)) => {
            let mut text = format!("{} · {} 条", metadata.name, dictionary.len());
            if builtin {
                text.push_str(" · 随包");
            } else if !metadata.license.is_empty() {
                text.push_str(&format!(" · {}", metadata.license));
            }
            (text, false)
        }
        // 后缀不在支持列表里：多半是把别的格式直接拷进来了 —— 指路，别让人猜。
        Err(_) if !supported(path) => (
            format!("{stem}（后缀不认识，点下面「导入词库…」转换）"),
            true,
        ),
        Err(_) => (format!("{stem}（读不了：文件损坏或格式不对）"), true),
    }
}

/// 后缀在不在「能直接放进目录」的那张表里（与 `extra_dictionaries` 的扫描表一致）。
fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e, "qj" | "tsv" | "yaml" | "yml" | "txt"))
}

/// 一本词库一行：复选框 + 可选的「移除」。
fn dict_row(
    stem: &str,
    label: String,
    enabled: bool,
    broken: bool,
    toggle: impl Fn(bool) -> Message + 'static,
    remove: Option<Message>,
    context: &mut ViewContext<Settings>,
) -> KeyedView {
    let check = CheckBox::new()
        .is_checked(enabled)
        .is_enabled(!broken)
        .on_is_checked_changed(context.callback(toggle))
        .content(label);
    let row = match remove {
        Some(message) => StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(12.0)
            .children((
                check,
                Button::new()
                    .on_click(context.message(message))
                    .content("移除"),
            )),
        None => check,
    };
    KeyedView::new(stem.to_owned(), row)
}

fn bundled_list(settings: &Settings, context: &mut ViewContext<Settings>) -> View {
    let Some(dir) = repo_resource("data/generated/dicts") else {
        return note("没找到随包领域词库目录（安装布局待定）。");
    };
    let dicts = extra_dictionaries::list(&dir);
    if dicts.is_empty() {
        return note("随包领域词库目录是空的。");
    }
    let mut rows: Vec<KeyedView> = Vec::with_capacity(dicts.len());
    for (stem, path) in dicts {
        let (label, broken) = row_label(&path, &stem, true);
        let enabled = settings.config.dictionaries.is_domain_enabled(&stem);
        let for_msg = stem.clone();
        rows.push(dict_row(
            &stem,
            label,
            enabled,
            broken,
            move |on| Message::ToggleDomain(for_msg.clone(), on),
            None,
            context,
        ));
    }
    StackPanel::new().spacing(6.0).keyed_children(rows)
}

fn user_list(settings: &Settings, context: &mut ViewContext<Settings>) -> View {
    let dicts = extra_dictionaries::list(&user_dir(settings));
    if dicts.is_empty() {
        return note(
            "这里还没有词库。点下面「导入词库」加一本，或把文件直接放进 \
             %APPDATA%\\Qingjian\\dicts（认 .qj / .tsv / Rime .dict.yaml / .txt，放进去就生效）。",
        );
    }
    let mut rows: Vec<KeyedView> = Vec::with_capacity(dicts.len());
    for (stem, path) in dicts {
        let (label, broken) = row_label(&path, &stem, false);
        let enabled = settings.config.dictionaries.is_enabled(&stem);
        let for_msg = stem.clone();
        let remove = Message::RemoveUserDict(stem.clone());
        rows.push(dict_row(
            &stem,
            label,
            enabled,
            broken,
            move |on| Message::ToggleUserDict(for_msg.clone(), on),
            Some(remove),
            context,
        ));
    }
    StackPanel::new().spacing(6.0).keyed_children(rows)
}

/// 目录里**没被认出来**的文件（后缀不在支持列表里）：列出来并指路，别静默忽略 ——
/// 2026-09-18 的 bug 就是 `.dict.yaml` 放进去了、界面里一个字都不显示。
/// 没有这类文件时返回 `None`（这一节就不出现）。
fn ignored_list(settings: &Settings) -> Option<View> {
    let dir = user_dir(settings);
    let known: Vec<PathBuf> = extra_dictionaries::list(&dir)
        .into_iter()
        .map(|(_, path)| path)
        .collect();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && !known.contains(path))
        .filter_map(|path| path.file_name()?.to_str().map(str::to_owned))
        .collect();
    if names.is_empty() {
        return None;
    }
    names.sort();
    let rows: Vec<KeyedView> = names
        .into_iter()
        .map(|name| {
            let text = format!("{name} —— 这个后缀不认识，点「导入词库…」转换一下");
            KeyedView::new(name, note(&text))
        })
        .collect();
    Some(StackPanel::new().spacing(6.0).keyed_children(rows))
}

pub(crate) fn view(settings: &Settings, context: &mut ViewContext<Settings>) -> View {
    let mut rows: Vec<View> = vec![
        note(
            "随包的基础词库始终启用，不在这里。这里管随包领域词库的开关与导入词库的开关 / 移除。改完自动生效。",
        ),
        TextBlock::new()
            .text("随包领域词库")
            .font_weight(FontWeight::SEMI_BOLD)
            .into(),
        bundled_list(settings, context),
        TextBlock::new()
            .text("导入的词库")
            .font_weight(FontWeight::SEMI_BOLD)
            .into(),
        user_list(settings, context),
    ];
    if let Some(ignored) = ignored_list(settings) {
        rows.push(ignored);
    }
    rows.push(
        StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(12.0)
            .children((
                Button::new()
                    .on_click(context.message(Message::ImportDictionary))
                    .content("导入词库…"),
                note("接受青简 TSV、Rime .dict.yaml、.qj、.txt；导入会转成 <名字>.qj 放进上面的目录。"),
            )),
    );
    rows.push(note(&settings.dictionary_status));
    let body = StackPanel::new().spacing(12.0).keyed_children(
        rows.into_iter()
            .enumerate()
            .map(|(index, view)| KeyedView::new(index.to_string(), view)),
    );
    page("词库", body)
}

/// 挪进 `dicts\removed`，不真删（与 macOS 一致）。
pub(crate) fn remove_user_dict(settings: &mut Settings, stem: &str) {
    let dir = user_dir(settings);
    let Some((_, path)) = extra_dictionaries::list(&dir)
        .into_iter()
        .find(|(name, _)| name == stem)
    else {
        return;
    };
    let removed = dir.join("removed");
    if let Err(error) = std::fs::create_dir_all(&removed) {
        settings.dictionary_status = format!("移除失败：{error}");
        return;
    }
    if let Some(file_name) = path.file_name() {
        settings.dictionary_status = match std::fs::rename(&path, removed.join(file_name)) {
            Ok(()) => format!("已移除「{stem}」，输入法将自动更新。"),
            Err(error) => format!("移除失败：{error}"),
        };
    }
}

/// 多选词库，逐个转换并汇总结果；成功项的开关一次写回。
pub(crate) fn import(settings: &mut Settings) {
    let Some(sources) = rfd::FileDialog::new()
        .add_filter("词库文件", &["tsv", "yaml", "yml", "txt", "qj"])
        .add_filter("所有文件", &["*"])
        .set_title("导入词库")
        .pick_files()
    else {
        return;
    };
    let dir = user_dir(settings);
    let mut disabled = settings.config.dictionaries.disabled.clone();
    let mut results = Vec::new();
    let mut succeeded = 0;
    for source in &sources {
        match qingjian_core::dictionary::import::import(source, &dir) {
            Ok(imported) => {
                if let Some(stem) = imported.path.file_stem().and_then(|s| s.to_str()) {
                    disabled.retain(|name| name != stem);
                }
                succeeded += 1;
                results.push(format!(
                    "已导入「{}」，共 {} 条。",
                    imported.name, imported.entries
                ));
            }
            Err(error) => {
                let message = format!("{} 导入失败：{error}", source.display());
                crate::log::warn(&message);
                results.push(message);
            }
        }
    }
    let mut summary = format!(
        "导入完成：成功 {} 个，失败 {} 个。",
        succeeded,
        sources.len() - succeeded
    );
    if disabled != settings.config.dictionaries.disabled
        && let Err(error) = qingjian_platform::Config::set_array(
            &settings.path,
            "dictionaries",
            "disabled",
            &disabled,
        )
    {
        results.push(format!(
            "自动启用失败：{error}。此前关闭的词库需手动勾选启用。"
        ));
    } else if succeeded > 0 {
        summary.push_str("输入法将自动加载。");
    }
    settings.dictionary_status = format!("{summary}\n{}", results.join("\n"));
}
