use std::time::Instant;

/// 日志方向/来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Rx,
    Tx,
    Sys,
    Err,
}

/// 一条格式化后的输出行
#[derive(Debug, Clone)]
pub struct LogLine {
    pub ts: Option<String>,
    pub dir: Dir,
    pub body: String,
}

/// 行组装器：把串口字节流按换行切分成"日志行"，
/// 未完成行暂存 pending，超时后强制刷行。
pub struct LineAssembler {
    pending: String,
    /// 最近一次产生输出（切出完整行或刷出 pending）的时间，用于流式超时刷行
    last_flushed: Instant,
    /// pending 超过该字节数时即使未到超时也强制刷出，防止持续无换行数据流无限增长
    max_pending: usize,
}

impl LineAssembler {
    pub fn new() -> Self {
        Self {
            pending: String::new(),
            last_flushed: Instant::now(),
            max_pending: 16 * 1024,
        }
    }

    /// 输入解码后的文本，返回完整行（不含行尾换行）
    pub fn push(&mut self, text: &str, now: Instant) -> Vec<String> {
        self.pending.push_str(text);
        let mut out = Vec::new();
        // 单趟切分：仅对含换行的前缀做 split_inclusive，剩余部分留在 pending。
        // 避免原实现反复 drain(..=idx) 导致的 O(n²) 字符移位。
        if let Some(pos) = self.pending.rfind('\n') {
            let tail = self.pending.split_off(pos + 1);
            for line in self.pending.split_inclusive('\n') {
                let mut line = line;
                if line.ends_with('\n') {
                    line = &line[..line.len() - 1];
                }
                if line.ends_with('\r') {
                    line = &line[..line.len() - 1];
                }
                out.push(line.to_string());
            }
            self.pending = tail;
            // 本次成功切出了完整行，刷新"上次输出"时间（pending 剩余是正常的未完成半行）
            self.last_flushed = now;
        }
        out
    }

    /// 刷出待处理数据。触发条件：距上次产生输出已超过 `flush_ms`，或 pending
    /// 超过 `max_pending`（防止持续无换行数据流无限累积）。
    /// 注意：判定基于"距上次输出"而非"距上次输入"，否则持续流入的数据流会
    /// 不断刷新时间戳导致 pending 永不刷出（RX 增长但输出区不显示）。
    pub fn take_pending_if_stale(&mut self, flush_ms: u64, now: Instant) -> Option<String> {
        if self.pending.is_empty() {
            return None;
        }
        let idle = now.duration_since(self.last_flushed).as_millis() as u64 >= flush_ms;
        let overlong = self.pending.len() >= self.max_pending;
        if idle || overlong {
            self.last_flushed = now;
            Some(std::mem::take(&mut self.pending))
        } else {
            None
        }
    }
}

impl Default for LineAssembler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn now() -> Instant {
        Instant::now()
    }

    #[test]
    fn single_complete_line() {
        let mut a = LineAssembler::new();
        let out = a.push("hello\n", now());
        assert_eq!(out, vec!["hello".to_string()]);
    }

    #[test]
    fn multiple_lines_in_one_chunk() {
        let mut a = LineAssembler::new();
        let out = a.push("a\nb\nc\n", now());
        assert_eq!(out, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn partial_line_held_until_newline() {
        let mut a = LineAssembler::new();
        // 首块无换行：不产生输出
        assert!(a.push("hello", now()).is_empty());
        // 补充换行：才吐出完整行
        assert_eq!(a.push(" world\n", now()), vec!["hello world".to_string()]);
    }

    #[test]
    fn multiple_chunks_within_one_line() {
        let mut a = LineAssembler::new();
        assert!(a.push("你", now()).is_empty());
        assert!(a.push("好", now()).is_empty());
        assert_eq!(a.push("\n", now()), vec!["你好".to_string()]);
    }

    #[test]
    fn strips_cr_and_lf() {
        let mut a = LineAssembler::new();
        let out = a.push("line\r\n", now());
        assert_eq!(out, vec!["line".to_string()]);
    }

    #[test]
    fn stale_pending_flushed() {
        let mut a = LineAssembler::new();
        let t0 = Instant::now();
        a.push("half", t0);
        // 距上次输出未到 flush_ms：不刷出
        assert!(a.take_pending_if_stale(200, t0).is_none());
        // 超过 flush_ms：刷出
        assert_eq!(
            a.take_pending_if_stale(200, t0 + Duration::from_millis(250)),
            Some("half".to_string())
        );
    }

    #[test]
    fn no_pending_returns_none() {
        let mut a = LineAssembler::new();
        assert!(a.take_pending_if_stale(200, now()).is_none());
    }

    #[test]
    fn pending_cleared_after_flush() {
        let mut a = LineAssembler::new();
        let t0 = Instant::now();
        a.push("data", t0);
        let flushed = a.take_pending_if_stale(200, t0 + Duration::from_millis(300));
        assert_eq!(flushed, Some("data".to_string()));
        // 刷出后无遗留
        assert!(a.take_pending_if_stale(200, t0 + Duration::from_millis(500)).is_none());
    }

    /// 核心修复：持续无换行的数据流（每次 push 时间戳不断刷新）也应周期性刷出，
    /// 否则 RX 计数持续增长但 pending 永不刷出（输出区不显示）。
    #[test]
    fn continuous_stream_flushed_periodically() {
        let mut a = LineAssembler::new();
        let t0 = Instant::now();
        // 模拟持续数据流：每 50ms 推入一块无换行数据，200ms 内刷出一次
        for i in 0..6 {
            let t = t0 + Duration::from_millis(i * 50);
            a.push("stream", t);
        }
        // 距上次输出（切行/刷出）超过 200ms 时应刷出当前累积数据
        let flushed = a.take_pending_if_stale(200, t0 + Duration::from_millis(300));
        assert!(flushed.is_some(), "持续数据流应周期性刷出");
        assert_eq!(flushed.unwrap(), "streamstreamstreamstreamstreamstream");
        // 刷出后 pending 清空
        assert!(a.take_pending_if_stale(200, t0 + Duration::from_millis(500)).is_none());
    }

    /// 超过 max_pending 长度时即使未到超时也强制刷出，防止内存无限增长
    #[test]
    fn overlong_pending_force_flushed() {
        let mut a = LineAssembler::new();
        let t0 = Instant::now();
        let big = "x".repeat(16 * 1024 + 100);
        a.push(&big, t0);
        // 未到 flush_ms，但 pending 超长：立即刷出
        let flushed = a.take_pending_if_stale(200, t0 + Duration::from_millis(50));
        assert_eq!(flushed, Some(big));
    }
}
