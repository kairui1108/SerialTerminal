pub mod engine;
pub mod logline;

use logline::Dir;

/// 串口读线程 -> 主线程的事件
pub enum SerialEvent {
    Data(Vec<u8>),
    Error(String),
    /// 回放线程回显的记录（用于在输出区展示回放过程）
    Replay(Dir, String),
}

/// 写线程 -> 主线程的写结果
pub enum WriteResult {
    Ok(usize),
    Err(String),
}
