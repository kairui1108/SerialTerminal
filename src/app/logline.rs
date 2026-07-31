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
    last_activity: Option<Instant>,
}

impl LineAssembler {
    pub fn new() -> Self {
        Self {
            pending: String::new(),
            last_activity: None,
        }
    }

    /// 输入解码后的文本，返回完整行（不含行尾换行）
    pub fn push(&mut self, text: &str, now: Instant) -> Vec<String> {
        self.pending.push_str(text);
        self.last_activity = Some(now);
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
        }
        out
    }

    /// 若半行超时未完成，则强制刷出
    pub fn take_pending_if_stale(&mut self, flush_ms: u64, now: Instant) -> Option<String> {
        if self.pending.is_empty() {
            return None;
        }
        let stale = self
            .last_activity
            .is_none_or(|t| now.duration_since(t).as_millis() as u64 >= flush_ms);
        if stale {
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
        assert!(a.take_pending_if_stale(200, t0).is_none());
        assert!(
            a.take_pending_if_stale(200, t0 + Duration::from_millis(250))
                .is_some()
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
        assert!(
            a.take_pending_if_stale(200, t0 + Duration::from_millis(500))
                .is_none()
        );
    }
}
