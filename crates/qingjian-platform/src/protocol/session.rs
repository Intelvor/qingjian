use serde::{Deserialize, Serialize};

/// 一次输入会话的标识。DLL 每进入一个 TSF 文档（`ITfContext`）就开一个会话，Server 按它分派状态。
///
/// 由 DLL 分配，Server 只做键、不解释其数值 —— 但**同一个 Server 的所有会话里必须唯一**：
/// 一个 Server 服务本机所有应用，会话表是全局一张。Windows 侧用「进程号 `<< 32 | 线程号`」，
/// 因为 TSF 激活时传进来的 client id 不是全局唯一的（实测那几个值在完全不同的进程之间反复出现），
/// 拿它当会话标识会让后开的应用把先开的顶掉。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SessionId(pub u64);
