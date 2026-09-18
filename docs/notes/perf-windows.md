# 性能（Windows）：首轮基线（2026-09-18）

`performance.md` 里的数字都来自 M 系列 Mac、release 构建；这篇记录 **Windows 真机的首轮基线**，作为以后 Windows 侧改动的对照起点。
测量没有改任何代码逻辑，用的都是现有测量手段。

**机器**：Intel i7-14650HX · 16 核 / 24 线程 · 基频 ~2.2 GHz · 32 GB 内存 · Windows 11 专业工作站（Build 26200）。
数据 = 本仓库源码（release 重编）+ 已装机的输入法实例（`D:\Program Files\Qingjian`，默认配置 model + fuzzy + cloud 全开）。

## 怎么量

- 引擎逐键 / 启动：`target\release\qingjian-cli.exe --typing <拼音>`，日志走 stderr（`Engine 就绪 total_ms=…`、各表 `load_ms`）。
- Server 启动：从已装机实例的滚动日志（`%LOCALAPPDATA%\Qingjian\logs\server-*.log`）按时间戳重建分阶段耗时——不动正在跑的进程。
- 本地模型：`--neural data/model/model.qjm`（sync 模式，模型重打分亏在 `rank` 里）。
- Cloud 联想的 `elapsed_ms` 直接取自真实日志样本，非本机 CPU 性能项，仅作参考。

## 引擎基线（每键热路径）—— 无压力

release CLI 逐键计时（与输入法共享 dict.qj + lm.qj + 同一查询路径）：

| 输入 | 键数 | 平均 | 最慢 |
|---|---|---|---|
| `nihao` | 5 | 1.49 ms | 4.78 ms |
| `woshizhongguorenwoaiwodeguojia` | 30 | 0.93 ms | 5.44 ms |

最慢一档落在长句前几键的重切分（parse ~1–2 ms），仍远低于每键 10 ms 预算。**引擎查询在本机不是瓶颈。**

## Server 真实装机启动（分阶段）

从正在运行的装机实例重建（默认配置，`language=en`、`model_enabled=true`）：

| 阶段 | 耗时 |
|---|---|
| 词库装配（dict.qj mmap） | 6 ms |
| 语言模型（lm.qj） | 12 ms |
| 英中释义 44924 条 + 英文词表 + emoji + 个人释义 66 条 | ~60 ms |
| → 引擎「就绪」（从词库起算） | **~80 ms** |
| 渲染器初始化（Segoe UI，候选窗 + 状态条贴为该帧） | 1 ms |
| 本地整句模型 加载 + 预热（53 MB） | **92 ms** |

**结论**：
- 本地模型预热是启动最大单项，约占「到全部就绪 ~170 ms」的一半以上。
- 「就绪」日志在模型预热之前发出（加载走后台），用户能开始打字的感知时间其实是 ~80 ms；
  把模型预热改成可中断 / 推迟到空闲可进一步压启动感知时间。
- 与 mac 基数（`performance.md`：Engine 就绪 50 ms 量级）方向一致：词库 / 语言模型都进了 `.qj`，个位数 ms。

## 本地整句模型（CPU，Windows 无 Metal，candle 走纯 Rust gemm）

- 加载：CLI `load_ms=135` / Server 预热 `total_ms=92`。
- sync 逐键重打分（`--neural`，8 条候选，前文随句长）：

| 输入 | rank | total |
|---|---|---|
| `woshizhongguoren`（6 音节） | 34 ms | 34.6 ms |
| `woshizhongguorenwoaiwodeguojia`（10 音节） | 47.8 ms | 48.3 ms |

- 逐键重打分约 **35–48 ms**，随前文 64 字 × 8 条继续放大。在输入法里走**异步 + 防抖 80 ms**，不卡键，
  是「停键后候选重排到位」的延迟来源。对比 mac（Metal 64 字前文 × 8 条 25 ms，见 `performance.md` 第八轮），
  Windows CPU 是明显落后的一个维度，前文自适应 / 减路径是下一步最值得做的。

## 未覆盖（需真实键盘会话，headless 量不到）

- **IPC**:管道一问一答往返 + 每键 JSON 帧成本（需 TSF DLL 在应用里实际敲字）。
- **候选窗每帧 → `UpdateLayeredWindow` 全像素重绘**（需有可视候选窗驱动到屏幕）。

## 判读

- 引擎热路径、装配全部达标，瓶颈不在引擎。
- 本机两处明确热点：①本地模型预热 ≈92 ms（启动最大项）；②逐键重打分 ≈35–48 ms（异步，体验瓶颈）。
- Cloud 联想真实耗时 0.4–1.2 s、结果常 0–3 个词，异步不阻塞按键，属外部延迟，不在本机 CPU 性能范围内。