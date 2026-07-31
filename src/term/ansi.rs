//! ANSI 转义序列解析：把设备输出中的 SGR 颜色码转换为 ratatui 样式，
//! 其余控制序列（光标移动等）直接剥离，避免转义码泄漏为可见文本导致行列错乱。

use ratatui::prelude::{Color, Modifier, Span, Style};

/// 解析文本中的 ANSI 转义序列，返回带样式的 spans。
/// `default` 为无转义时的默认样式（如主题前景色）。
pub fn ansi_to_spans(text: &str, default: Style) -> Vec<Span<'static>> {
    let mut spans: Vec<Span> = Vec::new();
    let mut buf = String::new();
    let mut style = default;
    let bytes = text.as_bytes();
    let mut i = 0usize;

    while i < text.len() {
        if bytes[i] == 0x1b {
            // 先刷出已积累的普通文本
            if !buf.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut buf), style));
            }
            // CSI 序列: ESC [ <params> <final byte>
            if i + 1 < text.len() && bytes[i + 1] == b'[' {
                let mut j = i + 2;
                while j < text.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                    j += 1;
                }
                if j < text.len() {
                    if bytes[j] == b'm' {
                        // SGR: 应用到当前样式
                        let params = &text[i + 2..j];
                        style = apply_sgr(style, params, default);
                    }
                    // 其他 CSI（光标移动、清行等）直接剥离
                    i = j + 1;
                } else {
                    // 未闭合的 CSI，丢弃剩余
                    i = text.len();
                }
            } else {
                // 单独的 ESC，丢弃
                i += 1;
            }
        } else {
            let ch = text[i..].chars().next().unwrap();
            buf.push(ch);
            i += ch.len_utf8();
        }
    }
    if !buf.is_empty() {
        spans.push(Span::styled(buf, style));
    }
    spans
}

/// 应用 SGR 参数到样式（支持 0/reset、1/bold、前景色、亮色、背景色）
fn apply_sgr(mut style: Style, params: &str, default: Style) -> Style {
    if params.is_empty() {
        // ESC[m 等价于 reset
        return default;
    }
    for p in params.split(';') {
        let n = match p.parse::<u16>() {
            Ok(n) => n,
            Err(_) => continue,
        };
        match n {
            0 => style = default,
            1 => style = style.add_modifier(Modifier::BOLD),
            22 => style = style.remove_modifier(Modifier::BOLD),
            7 => style = style.add_modifier(Modifier::REVERSED),
            27 => style = style.remove_modifier(Modifier::REVERSED),
            30..=37 => style = style.fg(sgr_fg(n)),
            90..=97 => style = style.fg(sgr_bright_fg(n)),
            39 => style = default, // 默认前景
            40..=47 => style = style.bg(sgr_fg(n - 10)),
            100..=107 => style = style.bg(sgr_bright_fg(n - 10)),
            _ => {}
        }
    }
    style
}

fn sgr_fg(n: u16) -> Color {
    match n {
        31 => Color::Red,
        32 => Color::Green,
        33 => Color::Yellow,
        34 => Color::Blue,
        35 => Color::Magenta,
        36 => Color::Cyan,
        37 => Color::White,
        _ => Color::Black,
    }
}

fn sgr_bright_fg(n: u16) -> Color {
    match n {
        91 => Color::LightRed,
        92 => Color::LightGreen,
        93 => Color::LightYellow,
        94 => Color::LightBlue,
        95 => Color::LightMagenta,
        96 => Color::LightCyan,
        97 => Color::Gray,
        _ => Color::DarkGray,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ansi_and_keeps_text() {
        let text = "\x1b[0;32mI (71264) NimBLE: att_handle=25\x1b[0m";
        let spans = ansi_to_spans(text, Style::new().fg(Color::White));
        let joined: String = spans
            .iter()
            .map(|s| s.content.as_ref().to_string())
            .collect();
        assert_eq!(joined, "I (71264) NimBLE: att_handle=25");
    }

    #[test]
    fn applies_green_color() {
        let text = "\x1b[0;32mHELLO\x1b[0m";
        let spans = ansi_to_spans(text, Style::new());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(Color::Green));
        assert_eq!(spans[0].content, "HELLO");
    }

    #[test]
    fn handles_plain_text() {
        let text = "no ansi here";
        let spans = ansi_to_spans(text, Style::new());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "no ansi here");
    }

    #[test]
    fn strips_cursor_moves() {
        let text = "A\x1b[2KB";
        let spans = ansi_to_spans(text, Style::new());
        let joined: String = spans
            .iter()
            .map(|s| s.content.as_ref().to_string())
            .collect();
        assert_eq!(joined, "AB");
    }

    #[test]
    fn handles_multibyte_around_ansi() {
        let text = "你\x1b[0m好";
        let spans = ansi_to_spans(text, Style::new());
        let joined: String = spans
            .iter()
            .map(|s| s.content.as_ref().to_string())
            .collect();
        assert_eq!(joined, "你好");
    }
}
