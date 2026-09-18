# 图标

- `logo.png`（866×866，带透明通道）：应用图标源文件。`apps/macos/scripts/bundle.sh` 打包时用 `sips` + `iconutil`
  生成 `Qingjian.icns`（应用图标）和 `qingjian-menu.tiff`（输入法菜单 / 菜单栏用的 16pt 双分辨率图标），
  生成物不进仓库。
- `windows/mode-{zh,en,caps,zhuyin,pinyin,shuangpin,wubi}.svg`：Windows 任务栏模式图标的源文件
  （16×16 画布，单色）：中 / 英 / A / 注 / 拼 / 双 / 五。`windows/render-mode-icons.sh` 用 rsvg-convert + magick
  栅格化成 16 / 20 / 24 / 32 四档的 8 位 alpha 蒙版，写到 `apps/windows/tsf/resources/mode/`，DLL 用
  `include_bytes!` 嵌入、运行时按任务栏深浅色填色（`com/mode/icon.rs`）。改了 svg 重跑脚本，生成物随仓库提交。
- 中 / 英 / A 三张是设计稿手画的轮廓；注 / 拼 / 双 / 五 没有设计稿，由 `windows/render-mode-icons.ps1`
  取 Noto Sans SC Black 的字形轮廓生成（笔画重量与设计稿最接近，ink 高度 13.5/16，对齐「中」14、「英」13）。
  这个 .ps1 也顺便在 Windows 上直接出四档蒙版，没有 rsvg-convert / magick 时用它：
  `... -Char 0x62FC -Name pinyin`（-Char 是码点、-Name 是文件名前缀）。
