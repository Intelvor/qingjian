use std::sync::mpsc::Sender;

use qingjian_platform::protocol::{ClientMessage, ServerMessage};

use crate::dispatch::{CandidateEvent, StatusEvent};

/// 工人线程（独占 [`crate::dispatch::Router`]）的一件活：DLL 的一条消息，或候选窗 / 状态条上的一次操作。
pub enum Work {
    /// 某条连接收到的消息 + 回结果的通道（`None` 表示不用回话）。
    Client(ClientMessage, Sender<Option<ServerMessage>>),

    /// UI 线程发来的状态条操作。
    Status(StatusEvent),

    /// UI 线程发来的候选窗口操作（鼠标点选）。
    Candidate(CandidateEvent),

    /// UI 线程发来的「前台窗口变了」（WinEvent 钩子）：前台窗口的归属线索 `(线程 id, 进程 id)`，
    /// 按可信度从高到低排。Router 拿它对会话表，认出哪个会话在前台（悬浮状态条跟它走）。
    Foreground(Vec<(u32, u32)>),
}
