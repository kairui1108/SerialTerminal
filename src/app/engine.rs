use crate::app::logline::{Dir, LineAssembler, LogLine};
use crate::app::{SerialEvent, WriteResult};
use crate::cli::Cli;
use crate::config::{ConfigManager, RecentDevice};
use crate::serial::codec::{Encoding, StreamDecoder, StreamEncoder};
use crate::serial::port::{PortDevice, PortParams, list_ports};
use crate::term::ansi;
use crate::term::ui::Theme;

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::{DefaultTerminal, Frame};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};
use tui_textarea::{Input, TextArea};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendMode {
    Line,
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TsMode {
    Off,
    Short,
    Iso,
}

/// 全部命令及其说明（补全候选与帮助共用）
const COMMANDS: &[(&str, &str)] = &[
    (":port", "<p> 连接串口"),
    (":connect", "<p> 同 :port"),
    (":close", "关闭串口"),
    (":disconnect", "同 :close"),
    (":list", "列出可用串口"),
    (":settings", "打开设置面板"),
    (":config", "同 :settings"),
    (":status", "显示连接状态"),
    (":baud", "<n> 切换波特率"),
    (":databits", "<5-8> 数据位"),
    (":stopbits", "<1|2> 停止位"),
    (":parity", "<none|odd|even> 校验位"),
    (":flow", "<none|soft|hard> 流控"),
    (":encoding", "<utf8|gbk|auto|raw> 编码"),
    (":mode", "<line|direct> 发送模式"),
    (":theme", "[主题] 切换主题"),
    (":hex", "[41 42 43] Hex视图/发送"),
    (":text", "文本视图"),
    (":ts", "<on|off|iso> 时间戳"),
    (":tail", "回到最新输出"),
    (":top", "回到输出顶部"),
    (":live", "同 :tail"),
    (":follow", "同 :tail"),
    (":up", "上滑历史"),
    (":down", "下滑"),
    (":scroll", "<up|down|行数> 滚动"),
    (":clear", "清屏"),
    (":search", "<关键词|off> 搜索/清除高亮"),
    (":find", "同 :search"),
    (":send", "<文本> 发送"),
    (":log", "<on|off> 日志"),
    (":recent", "[序号] 最近设备"),
    (":reconnect", "[端口|off] 断线自动重连"),
    (":auto", "同 :reconnect"),
    (":export", "[文件] 导出日志"),
    (":replay", "<文件 [倍率]|stop> 回放/停止"),
    (":help", "帮助"),
    (":quit", "退出"),
    (":exit", "同 :quit"),
];

/// 断线自动重连的尝试间隔
const RECONNECT_INTERVAL: Duration = Duration::from_secs(1);

/// 参数级补全：命令名 -> 可选参数值列表（供 Tab 补全参数）
fn param_values(cmd: &str) -> &'static [&'static str] {
    match cmd {
        ":baud" => &[
            "300", "600", "1200", "2400", "4800", "9600", "19200", "38400", "57600", "115200",
            "230400", "460800", "921600", "1000000", "2000000",
        ],
        ":encoding" => &["utf8", "gbk", "auto", "raw"],
        ":mode" => &["line", "direct"],
        ":parity" => &["none", "odd", "even"],
        ":stopbits" => &["1", "2"],
        ":databits" => &["5", "6", "7", "8"],
        ":flow" => &["none", "software", "hardware"],
        ":ts" => &["on", "off", "iso"],
        ":scroll" => &["up", "down"],
        ":log" => &["on", "off"],
        ":theme" => Theme::NAMES,
        _ => &[],
    }
}

/// 帮助面板分组：组名 -> 组内命令（渲染时从 COMMANDS 取描述）
const HELP_GROUPS: &[(&str, &[&str])] = &[
    (
        "连接管理",
        &[
            ":port",
            ":connect",
            ":close",
            ":disconnect",
            ":list",
            ":settings",
            ":config",
            ":status",
            ":baud",
            ":databits",
            ":stopbits",
            ":parity",
            ":flow",
        ],
    ),
    (
        "显示控制",
        &[
            ":tail",
            ":top",
            ":live",
            ":follow",
            ":scroll",
            ":up",
            ":down",
            ":clear",
            ":search",
            ":find",
            ":hex",
            ":text",
            ":ts",
            ":encoding",
            ":mode",
            ":theme",
        ],
    ),
    ("发送", &[":send"]),
    ("日志", &[":log", ":export", ":replay"]),
    (
        "其他",
        &[":recent", ":reconnect", ":help", ":quit", ":exit"],
    ),
];

/// 设置面板可选值
const BAUDS: [u32; 15] = [
    300, 600, 1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600, 1000000,
    2000000,
];
const DATA_BITS: [u8; 4] = [5, 6, 7, 8];
const STOP_BITS: [&str; 2] = ["1", "2"];
const PARITIES: [&str; 3] = ["none", "odd", "even"];
const FLOWS: [&str; 3] = ["none", "software", "hardware"];

/// 统一的命令前缀匹配，供动态补全与 Tab 补全复用
fn matching_commands(prefix: &str) -> Vec<&'static str> {
    COMMANDS
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| n.strip_prefix(':').unwrap_or("").starts_with(prefix))
        .collect()
}

fn cycle_value<T: Copy + PartialEq>(arr: &[T], cur: T, delta: i32) -> T {
    let i = arr.iter().position(|&x| x == cur).unwrap_or(0);
    let len = arr.len() as i32;
    arr[((i as i32 + delta + len) % len) as usize]
}

fn cycle_str<'a>(arr: &[&'a str], cur: &str, delta: i32) -> &'a str {
    let i = arr.iter().position(|&x| x == cur).unwrap_or(0);
    let len = arr.len() as i32;
    arr[((i as i32 + delta + len) % len) as usize]
}

/// 对正文中匹配关键词的片段应用高亮样式。
/// `spans` 为已解析 ANSI 样式的 spans（可能含多字符 span）。
/// 先拼接 spans 的纯文本（去 ANSI 后）用于匹配，再逐字符对应回原 spans，
/// 保证命中位置与显示内容严格一致（避免因 ANSI 转义导致高亮偏移）。
fn highlight_keyword(spans: Vec<Span<'static>>, kw: &str, hl: Style) -> Vec<Span<'static>> {
    // 1. 拼接 spans 的纯文本（与显示顺序完全一致）
    let plain: String = spans.iter().map(|s| s.content.as_ref()).collect();
    if plain.is_empty() || kw.is_empty() {
        return spans;
    }
    // 2. 在纯文本上找所有命中区间（字符索引）
    let plain_lower = plain.to_lowercase();
    let kw_lower = kw.to_lowercase();
    let plain_chars: Vec<char> = plain.chars().collect();
    let kw_chars: Vec<char> = kw_lower.chars().collect();
    let mut hit = vec![false; plain_chars.len()];
    let mut search_from = 0usize;
    while let Some(rel) = plain_lower[search_from..].find(&kw_lower) {
        let abs = search_from + rel;
        let start_char = plain[..abs].chars().count();
        for i in start_char..start_char + kw_chars.len() {
            if i < hit.len() {
                hit[i] = true;
            }
        }
        search_from = abs + kw_lower.len();
    }
    // 3. 逐字符重建 spans，命中字符用高亮样式
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut char_pos = 0usize;
    for span in spans {
        let base_style = span.style;
        for ch in span.content.chars() {
            let is_hit = char_pos < hit.len() && hit[char_pos];
            char_pos += 1;
            let style = if is_hit { hl } else { base_style };
            out.push(Span::styled(ch.to_string(), style));
        }
    }
    out
}

/// 将带样式的 spans 按终端宽度折成多段（保留每字符样式，供 ANSI 彩色文本折行）
fn wrap_spans(spans: &[Span<'_>], max_w: usize) -> Vec<Vec<Span<'static>>> {
    if max_w == 0 {
        // 无可用宽度：返回一个空行，由调用方兜底
        return vec![Vec::new()];
    }
    let mut rows: Vec<Vec<Span>> = vec![Vec::new()];
    let mut cur_w = 0usize;
    for span in spans {
        let style = span.style;
        for ch in span.content.chars() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(0);
            if cur_w + w > max_w && cur_w > 0 {
                rows.push(Vec::new());
                cur_w = 0;
            }
            rows.last_mut()
                .unwrap()
                .push(Span::styled(ch.to_string(), style));
            cur_w += w;
        }
    }
    rows
}

/// 拼接 exe 同目录路径（纯拼接，不创建/删除文件）。
/// 用于读取已有文件（如回放），绝不能调用可写性探测。
fn exe_dir_join(name: &str) -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        return dir.join(name);
    }
    std::path::PathBuf::from(name)
}

/// 列出应用目录（exe 同目录）下匹配前缀的 .log 文件，供 :replay 补全
fn replay_file_candidates(prefix: &str) -> Vec<String> {
    let mut files = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
        && let Ok(rd) = std::fs::read_dir(dir)
    {
        for e in rd.flatten() {
            let name = e.file_name();
            let name = name.to_string_lossy().to_string();
            if name.ends_with(".log") && name.starts_with(prefix) {
                files.push(name);
            }
        }
    }
    files.sort();
    files
}

/// 串口列表短 TTL 缓存：补全时用户连续按键会多次枚举系统设备，
/// 短时间（PORT_CACHE_TTL）内复用上次结果，降低 Windows USB 设备树枚举开销。
/// 仅用于补全提示；`:list`/设置面板等需要实时结果的路径仍直接调用 `list_ports()`。
const PORT_CACHE_TTL: Duration = Duration::from_millis(800);

/// 端口缓存：最近一次枚举的时间戳与结果
type PortCacheEntry = Option<(Instant, Vec<PortDevice>)>;

/// `:recent` 列表展示用的设备快照（索引/端口/波特率/数据位/停止位/校验位/编码/连接次数/是否当前）
type RecentSnapshot = (usize, String, u32, u8, String, String, String, u64, bool);

fn cached_port_candidates() -> Vec<PortDevice> {
    static CACHE: OnceLock<Mutex<PortCacheEntry>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let now = Instant::now();
    if let Ok(mut guard) = cache.lock() {
        // 命中未过期缓存
        let hit = guard
            .as_ref()
            .map(|(ts, _)| now.duration_since(*ts) < PORT_CACHE_TTL)
            .unwrap_or(false);
        if hit {
            return guard.as_ref().unwrap().1.clone();
        }
        // 缓存过期或为空：重新枚举并更新
        let ports = list_ports();
        *guard = Some((now, ports.clone()));
        ports
    } else {
        // 锁竞争（几乎不会发生）：直接实时枚举
        list_ports()
    }
}

/// 解析输出文件路径：优先存到 exe 同目录（便携、可预期），
/// exe 目录不可写时回退到当前工作目录。
fn resolve_output_path(name: &str) -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join(name);
        // 已存在则直接使用（绝不覆盖/删除已有文件）
        if candidate.exists() {
            return candidate;
        }
        // 不存在才尝试创建以验证 exe 目录可写
        if std::fs::write(&candidate, b"").is_ok() {
            let _ = std::fs::remove_file(&candidate);
            return candidate;
        }
    }
    std::path::PathBuf::from(name)
}

/// 端口名规范化：纯数字转 COM<n>，否则原样返回
fn normalize_port(s: &str) -> String {
    let s = s.trim();
    if let Ok(n) = s.parse::<u32>() {
        format!("COM{}", n)
    } else {
        s.to_string()
    }
}

/// 从日志行提取时间戳秒数（[HH:MM:SS.mmm] 格式），解析失败返回 None
fn extract_timestamp_seconds(line: &str) -> Option<f64> {
    let start = line.find('[')?;
    let end = line.find(']')?;
    if start >= end {
        return None;
    }
    let ts = &line[start + 1..end];
    let mut parts = ts.split(':');
    let h: f64 = parts.next()?.parse().ok()?;
    let m: f64 = parts.next()?.parse().ok()?;
    let s: f64 = parts.next()?.parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

/// 提取日志行正文（去掉 [时间戳] [RX|TX] 前缀）
fn extract_body(line: &str) -> Option<String> {
    // 格式: [HH:MM:SS.mmm] [TX ] 正文
    let after_ts = line.split_once("] ")?.1;
    let after_tag = after_ts.split_once("] ")?.1;
    Some(after_tag.to_string())
}

/// 还原日志中的转义序列（\r \n \t）
fn unescape(s: &str) -> String {
    s.replace("\\r", "\r")
        .replace("\\n", "\n")
        .replace("\\t", "\t")
}

/// 解析十六进制发送串（支持 "41 42 43"、"0x41 0x42"、"414243"）
fn parse_hex(s: &str) -> Option<Vec<u8>> {
    let compact: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if compact.is_empty() || !compact.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(compact.len() / 2);
    for i in (0..compact.len()).step_by(2) {
        out.push(u8::from_str_radix(&compact[i..i + 2], 16).ok()?);
    }
    Some(out)
}

pub struct Engine {
    cfg: ConfigManager,

    // 串口（读写句柄分离：读线程持有短超时读句柄，写线程持有长超时写句柄，互不阻塞）
    port_name: Option<String>,
    connected: bool,
    running: Arc<AtomicBool>,
    rx_handle: Option<thread::JoinHandle<()>>,
    tx_handle: Option<thread::JoinHandle<()>>,
    rx_tx: Sender<SerialEvent>,
    rx_events: Receiver<SerialEvent>,
    tx_sender: Sender<Vec<u8>>,
    tx_result_tx: Sender<WriteResult>,
    tx_results: Receiver<WriteResult>,

    // 参数
    baud: u32,
    databits: u8,
    stopbits: String,
    parity: String,
    flow: String,
    enc: Encoding,
    mode: SendMode,
    ts_mode: TsMode,
    no_tags: bool,
    flush_ms: u64,
    tx_on_enter: String,
    keep_input: bool,
    theme: Theme,

    // 编解码与行组装
    decoder: StreamDecoder,
    encoder: StreamEncoder,
    assembler: LineAssembler,

    // UI 状态
    lines: VecDeque<LogLine>,
    /// 每条日志的折行渲染缓存（与 lines 同步，draw 时增量维护）
    render_rows: VecDeque<Vec<Line<'static>>>,
    /// 渲染缓存对应的行宽（变化时整体重建）
    render_w: usize,
    /// 界面是否需要重绘（有事件/数据/输入时为 true，空闲时跳过 draw 降低 CPU）
    dirty: bool,
    textarea: TextArea<'static>,
    history: Vec<String>,
    hist_pos: usize,
    help_open: bool,
    /// 帮助面板滚动偏移
    help_scroll: usize,
    // 设置面板（串口参数配置弹窗）
    settings_open: bool,
    settings_sel: usize,
    port_candidates: Vec<PortDevice>,
    port_idx: usize,
    /// 是否跟随最新输出（滚动查看历史后置 false，:tail 恢复）
    follow: bool,
    /// 输出区可视行数（draw 时刷新，用于滚动计算）
    out_height: usize,
    /// 查看历史时视口顶部的日志行索引（锚点，新数据到达不影响当前位置）
    view_top: usize,
    /// 命令补全：当前 Tab 选中的候选索引
    completion_idx: usize,
    /// 命令补全：上次补全的命令前缀（变化时重置选中）
    last_cmd_prefix: String,
    /// 进入 Hex 视图前的编码（切回文本时恢复，避免固定回退 UTF-8）
    pre_hex_enc: Encoding,
    /// 当前搜索关键词（用于渲染高亮；空表示未搜索）
    search_kw: Option<String>,
    /// 当前搜索的匹配行索引列表（日志行下标）
    search_matches: Vec<usize>,
    /// 当前定位到的匹配位置（search_matches 中的下标）
    search_pos: usize,
    quit: bool,
    last_tick: Instant,

    // 统计
    rx_bytes: u64,
    tx_bytes: u64,

    // 日志文件
    /// 日志文件（带缓冲：高频 RX 写入不立即刷盘，避免同步磁盘 IO 阻塞主循环）
    log: Option<BufWriter<File>>,

    // ---- 断线自动重连 ----
    /// 断开后尝试重连的目标端口（None 表示不自动重连）
    reconnect_port: Option<String>,
    /// 下次重连尝试时间
    next_reconnect: Option<Instant>,
    /// 连续重连失败计数（用于限速提示）
    reconnect_attempts: u32,

    // ---- 日志回放 ----
    /// 回放线程句柄（运行中则发送键被禁用避免干扰）
    replay_handle: Option<thread::JoinHandle<()>>,
    /// 回放停止标志（:replay stop 置位后线程安全退出）
    replay_stop: Option<Arc<AtomicBool>>,
    /// 回放是否正在运行
    replaying: bool,
}

impl Engine {
    pub fn new(cli: Cli, cfg: ConfigManager) -> anyhow::Result<Self> {
        // 合并配置：CLI 显式值优先，其次配置文件。
        // 注意：clap 的默认值与"显式传入默认值"无法区分，
        // 因此"CLI 值 == 默认值"一律视为未指定，回退到配置文件。
        let baud = if cli.baud != 115200 {
            cli.baud
        } else {
            cfg.cfg.baud
        };
        let enc = Encoding::from_str_name(if cli.encoding != "utf8" {
            &cli.encoding
        } else {
            &cfg.cfg.encoding
        });
        let mode = if cli.mode == "direct" || cfg.cfg.mode == "direct" {
            SendMode::Direct
        } else {
            SendMode::Line
        };
        let ts_mode = match cli.ts.as_str() {
            "off" => TsMode::Off,
            "iso" => TsMode::Iso,
            _ => TsMode::Short,
        };
        let databits = if cli.databits != 8 {
            cli.databits
        } else {
            cfg.cfg.databits
        };
        let databits = if (5..=8).contains(&databits) {
            databits
        } else {
            8
        };
        let stopbits = if cli.stopbits != "1" {
            cli.stopbits.clone()
        } else {
            cfg.cfg.stopbits.clone()
        };
        let parity = if cli.parity != "none" {
            cli.parity.clone()
        } else {
            cfg.cfg.parity.clone()
        };
        let flow = if cli.flow != "none" {
            cli.flow.clone()
        } else {
            cfg.cfg.flow.clone()
        };
        let theme = Theme::from_name(if cli.theme != "github-dark" {
            &cli.theme
        } else {
            &cfg.cfg.theme
        });
        let history = cfg.cfg.history.clone();

        // 无配置对应项的 CLI 参数在 move 进 struct 前提取，避免"先硬编码再覆盖"的中间态
        let flush_ms = cli.flush_ms;
        let tx_on_enter = cli.tx_on_enter.clone();
        let keep_input = cli.keep_input;
        let no_tags = cli.no_tags;

        // RX 事件通道用有界容量：设备高速输出时读线程产生数据远快于主循环消费，
        // 无界通道会无限堆积导致内存增长并占满主循环（输出卡顿 + 命令输入失效）。
        // 有界通道配合读线程 send_timeout 背压，让主循环有窗口响应输入事件。
        let (rx_tx, rx_events) = bounded::<SerialEvent>(512);
        let (tx_sender, _) = unbounded::<Vec<u8>>();
        let (tx_result_tx, tx_results) = unbounded();
        let textarea = TextArea::default();
        // 提取 CLI 启动连接意图（struct 不再保存 cli）
        let cli_port = cli.port.clone();
        let cli_auto_connect = cli.auto_connect;

        let mut engine = Self {
            cfg,
            port_name: None,
            connected: false,
            running: Arc::new(AtomicBool::new(false)),
            rx_handle: None,
            tx_handle: None,
            rx_tx,
            rx_events,
            tx_sender,
            tx_result_tx,
            tx_results,
            baud,
            databits,
            stopbits,
            parity,
            flow,
            enc,
            mode,
            ts_mode,
            no_tags,
            flush_ms,
            tx_on_enter,
            keep_input,
            theme,
            decoder: StreamDecoder::new(enc),
            encoder: StreamEncoder::new(enc),
            assembler: LineAssembler::new(),
            lines: VecDeque::new(),
            render_rows: VecDeque::new(),
            render_w: 0,
            dirty: true,
            textarea,
            history,
            hist_pos: 0,
            view_top: 0,
            help_open: false,
            help_scroll: 0,
            settings_open: false,
            settings_sel: 0,
            port_candidates: Vec::new(),
            port_idx: 0,
            follow: true,
            out_height: 0,
            completion_idx: 0,
            last_cmd_prefix: String::new(),
            pre_hex_enc: enc,
            search_kw: None,
            search_matches: Vec::new(),
            search_pos: 0,
            quit: false,
            last_tick: Instant::now(),
            rx_bytes: 0,
            tx_bytes: 0,
            log: None,
            reconnect_port: None,
            next_reconnect: None,
            reconnect_attempts: 0,
            replay_handle: None,
            replay_stop: None,
            replaying: false,
        };

        // 默认不自动连接：弹出设置面板，由用户手动选择端口与参数后再连接。
        // 仅当显式指定 --port 或 --auto-connect 时才自动连接。
        if cli_port.is_some() || cli_auto_connect {
            let port_name = cli_port.clone().or_else(|| engine.cfg.cfg.port.clone());
            if let Some(p) = port_name {
                engine.open_port(&p, engine.baud);
            } else {
                let ports = list_ports();
                if let Some(first) = ports.first() {
                    engine.open_port(first.name(), engine.baud);
                } else {
                    engine.push_line(Dir::Err, "未检测到可用串口，请打开设置面板选择端口");
                }
            }
        } else {
            engine.push_line(
                Dir::Sys,
                "请在弹出的设置面板中选择端口与参数（输入 :settings 可再次打开）",
            );
            engine.settings_open = true;
        }
        // 端口候选列表 + 默认选中上次使用的端口
        engine.port_candidates = list_ports();
        let has_last_port = if let Some(cfg_port) = &engine.cfg.cfg.port {
            if let Some(i) = engine
                .port_candidates
                .iter()
                .position(|p| p.name() == cfg_port)
            {
                engine.port_idx = i;
                true
            } else {
                false
            }
        } else {
            false
        };
        // 已记住上次可用端口：默认聚焦到「连接」按钮（回车直接连接）；
        // 否则聚焦到端口字段让用户选择
        if engine.settings_open {
            engine.settings_sel = if has_last_port { 8 } else { 0 };
        }
        Ok(engine)
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        // 安装 panic 钩子：异常退出时恢复终端，避免遗留 raw mode/备用屏导致终端卡死
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            ratatui::restore();
            let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
            default_hook(info);
        }));

        let mut terminal = ratatui::init();
        // 启用鼠标捕获：支持滚轮滚动 + 鼠标按下时暂停跟随。
        // 注意：在 Windows Terminal 中，启用鼠标捕获后左键拖选由程序接收，
        // 复制文本请使用 Shift+拖选（WT 原生，不受 mouse mode 影响）。
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture);
        let res = self.main_loop(&mut terminal);
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
        ratatui::restore();
        self.stop_threads();
        if let Err(e) = self.save_cfg() {
            eprintln!("保存配置失败: {}", e);
        }
        res
    }

    fn main_loop(&mut self, terminal: &mut DefaultTerminal) -> anyhow::Result<()> {
        self.push_line(Dir::Sys, "输入 :help 查看全部命令");
        loop {
            // 记录本帧是否有事件，结合 dirty 标志决定是否重绘，空闲时显著降低 CPU
            let mut need_redraw = false;
            if crossterm::event::poll(Duration::from_millis(50))? {
                match crossterm::event::read()? {
                    crossterm::event::Event::Key(k) if k.kind == KeyEventKind::Press => {
                        self.handle_key(k);
                        need_redraw = true;
                    }
                    crossterm::event::Event::Mouse(m) => {
                        self.handle_mouse(m);
                        need_redraw = true;
                    }
                    crossterm::event::Event::Resize(_, _) => need_redraw = true,
                    _ => {}
                }
            }
            // 每帧最多消费 MAX_RX_PER_FRAME 条 RX 事件，避免一次处理全部积压
            // 导致本帧输入事件（poll/read）长时间得不到响应（命令输入失效）。
            // 剩余积压留到下一帧处理；读线程的 send_timeout 背压会自动降载。
            const MAX_RX_PER_FRAME: usize = 256;
            let mut rx_consumed = 0usize;
            while rx_consumed < MAX_RX_PER_FRAME {
                match self.rx_events.try_recv() {
                    Ok(ev) => {
                        rx_consumed += 1;
                        match ev {
                            SerialEvent::Data(b) => self.handle_rx_data(b),
                            SerialEvent::Replay(dir, body) => self.push_line(dir, body),
                            SerialEvent::Error(e) => {
                                self.push_line(Dir::Err, format!("{}（已断开）", e));
                                // 触发断线自动重连（若已设置 reconnect_port）
                                if self.reconnect_port.is_some() && self.connected {
                                    self.connected = false;
                                    self.next_reconnect = Some(Instant::now());
                                }
                            }
                            SerialEvent::Overflow(dropped) => self.push_line(
                                Dir::Err,
                                format!(
                                    "数据积压，已丢弃 {} 块 RX 数据（设备输出过快或 UI 繁忙）",
                                    dropped
                                ),
                            ),
                        }
                    }
                    Err(_) => break,
                }
            }
            while let Ok(res) = self.tx_results.try_recv() {
                match res {
                    WriteResult::Ok(n) => self.tx_bytes += n as u64,
                    WriteResult::Err(e) => self.push_line(
                        Dir::Err,
                        format!("发送失败: {}（设备可能繁忙未及时读取串口，请稍后重试）", e),
                    ),
                }
            }
            let now = Instant::now();
            if now.duration_since(self.last_tick) >= Duration::from_millis(500) {
                self.last_tick = now;
                self.tick(now);
                // 状态栏计数/半行刷新均需重绘
                need_redraw = true;
            }
            if need_redraw || self.dirty {
                terminal.draw(|f| self.draw(f))?;
                self.dirty = false;
            }
            if self.quit {
                break;
            }
        }
        Ok(())
    }

    // ---------- 串口管理 ----------

    fn open_port(&mut self, name: &str, baud: u32) {
        self.stop_threads();

        // 新建发送通道（写线程专用）
        let (tx_sender, tx_receiver) = unbounded::<Vec<u8>>();
        self.tx_sender = tx_sender;

        let params = PortParams {
            name: name.into(),
            baud,
            databits: self.databits,
            stopbits: self.stopbits.clone(),
            parity: self.parity.clone(),
            flow: self.flow.clone(),
        };
        match params.open() {
            // try_clone 复制读句柄：读句柄短超时轮询，写句柄长超时，
            // 避免设备繁忙时写入 60ms 即超时（ERROR_SEM_TIMEOUT）。
            Ok(mut p) => match p.try_clone() {
                Ok(mut rx) => {
                    let _ = rx.set_timeout(Duration::from_millis(60));
                    let _ = p.set_timeout(Duration::from_millis(2000));
                    self.port_name = Some(name.into());
                    self.baud = baud;
                    self.connected = true;
                    self.reset_codecs();
                    self.rx_bytes = 0;
                    self.tx_bytes = 0;
                    // 记录最近使用设备（供 :recent 快速切换）
                    self.record_recent(name, baud);
                    // 自动记住当前端口并启用断线自动重连（:reconnect off 可关闭）
                    if self.reconnect_port.as_deref() != Some(name) {
                        self.reconnect_port = Some(name.into());
                    }
                    self.next_reconnect = None;
                    self.reconnect_attempts = 0;
                    self.spawn_rx_thread(rx);
                    self.spawn_tx_thread(p, tx_receiver);
                    self.push_line(Dir::Sys, format!("已连接 {} @ {} baud", name, baud));
                }
                Err(e) => {
                    self.connected = false;
                    self.port_name = Some(name.into());
                    self.push_line(Dir::Err, format!("打开 {} 失败: {}", name, e));
                }
            },
            Err(e) => {
                self.connected = false;
                self.port_name = Some(name.into());
                self.push_line(Dir::Err, format!("打开 {} 失败: {}", name, e));
            }
        }
    }

    /// 记录最近使用设备：存在则更新参数/时间，否则插入到最前；最多保留 10 条
    fn record_recent(&mut self, port: &str, baud: u32) {
        let now = chrono::Local::now().to_rfc3339();
        if let Some(r) = self.cfg.cfg.recent.iter_mut().find(|r| r.port == port) {
            r.baud = baud;
            r.databits = self.databits;
            r.stopbits = self.stopbits.clone();
            r.parity = self.parity.clone();
            r.flow = self.flow.clone();
            r.encoding = self.enc.as_str_name().to_string();
            r.last_used = now;
            r.conn_count += 1;
        } else {
            let r = RecentDevice {
                port: port.to_string(),
                baud,
                databits: self.databits,
                stopbits: self.stopbits.clone(),
                parity: self.parity.clone(),
                flow: self.flow.clone(),
                encoding: self.enc.as_str_name().to_string(),
                last_used: now,
                conn_count: 1,
            };
            self.cfg.cfg.recent.insert(0, r);
        }
        // 按 last_used 降序 + 截断 10 条
        self.cfg
            .cfg
            .recent
            .sort_by(|a, b| b.last_used.cmp(&a.last_used));
        self.cfg.cfg.recent.truncate(10);
    }

    fn spawn_rx_thread(&mut self, mut port: Box<dyn serialport::SerialPort>) {
        self.running.store(true, Ordering::Relaxed);
        let running = self.running.clone();
        let tx = self.rx_tx.clone();
        self.rx_handle = Some(thread::spawn(move || {
            let mut buf = [0u8; 4096];
            let mut err_count = 0u32;
            let mut dropped = 0usize;
            while running.load(Ordering::Relaxed) {
                match port.read(&mut buf) {
                    Ok(n) => {
                        err_count = 0;
                        if n > 0 {
                            // 有界通道背压：优先等待主循环消费（200ms），
                            // 仍满则丢弃该块并提示，避免读线程永久阻塞或内存无限增长。
                            use crossbeam_channel::{SendTimeoutError, TrySendError};
                            match tx.send_timeout(
                                SerialEvent::Data(buf[..n].to_vec()),
                                Duration::from_millis(200),
                            ) {
                                Ok(()) => {}
                                Err(SendTimeoutError::Timeout(ev)) => match tx.try_send(ev) {
                                    Ok(()) => {}
                                    Err(TrySendError::Full(_)) => {
                                        dropped += 1;
                                        let _ = tx.try_send(SerialEvent::Overflow(dropped));
                                    }
                                    Err(TrySendError::Disconnected(_)) => break,
                                },
                                Err(SendTimeoutError::Disconnected(_)) => break,
                            }
                        }
                    }
                    // 空闲超时是正常现象：read 阻塞到 timeout 后返回
                    // ErrorKind::TimedOut，绝不能当作设备断开，否则空闲时会持续误报。
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => err_count = 0,
                    Err(e) => {
                        // 真实错误（如设备拔出）：连续 3 次确认后上报一次并退出读线程，
                        // 让出端口供自动重连接管，避免空转占用。
                        err_count += 1;
                        if err_count >= 3 {
                            let _ = tx.send(SerialEvent::Error(format!(
                                "串口读取异常（设备可能已断开）: {}",
                                e
                            )));
                            break;
                        }
                        thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        }));
    }

    /// 写线程：独占写句柄，阻塞写入不卡 UI；
    /// 写超时 2s，设备繁忙时等待排空，超时后通过结果通道上报。
    fn spawn_tx_thread(
        &mut self,
        mut port: Box<dyn serialport::SerialPort>,
        incoming: Receiver<Vec<u8>>,
    ) {
        let running = self.running.clone();
        let result_tx = self.tx_result_tx.clone();
        self.tx_handle = Some(thread::spawn(move || {
            use crossbeam_channel::RecvTimeoutError;
            while running.load(Ordering::Relaxed) {
                match incoming.recv_timeout(Duration::from_millis(100)) {
                    Ok(bytes) => {
                        let mut written = 0usize;
                        while written < bytes.len() {
                            match port.write(&bytes[written..]) {
                                Ok(n) if n > 0 => written += n,
                                Ok(_) => break,
                                Err(e) => {
                                    let _ = result_tx.send(WriteResult::Err(e.to_string()));
                                    break;
                                }
                            }
                        }
                        if written == bytes.len() {
                            let _ = result_tx.send(WriteResult::Ok(written));
                        }
                    }
                    // 超时后回到循环开头检查 running
                    Err(RecvTimeoutError::Timeout) => {}
                    // 通道断开（sender 全部 drop）后退出
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        }));
    }

    fn stop_threads(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        // 写线程由 recv_timeout + running 标志退出（最多 ~100ms + 一个写超时）
        if let Some(h) = self.tx_handle.take() {
            for _ in 0..400 {
                if h.is_finished() {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
        // 等待读线程退出（最长约一个读超时 60ms）
        if let Some(h) = self.rx_handle.take() {
            for _ in 0..60 {
                if h.is_finished() {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
    }

    fn reset_codecs(&mut self) {
        self.decoder = StreamDecoder::new(self.enc);
        self.encoder = StreamEncoder::new(self.enc);
        self.assembler = LineAssembler::new();
    }

    // ---------- 数据处理 ----------

    fn handle_rx_data(&mut self, bytes: Vec<u8>) {
        self.rx_bytes += bytes.len() as u64;
        self.dirty = true;
        let text = self.decoder.push(&bytes);
        if text.is_empty() {
            return;
        }
        for line in self.assembler.push(&text, Instant::now()) {
            self.push_line(Dir::Rx, line);
        }
    }

    fn tick(&mut self, now: Instant) {
        if let Some(line) = self.assembler.take_pending_if_stale(self.flush_ms, now) {
            self.push_line(Dir::Rx, line);
        }
        // 回放线程结束时复位状态（"回放完成"提示由回放线程发出，保证位于所有回显之后）
        if self.replaying
            && let Some(h) = &self.replay_handle
            && h.is_finished()
        {
            self.replaying = false;
            self.replay_stop = None;
            self.replay_handle = None;
        }
        // 断线自动重连：到达下次尝试时间且仍断开时重试
        if let Some(port) = self.reconnect_port.clone()
            && !self.connected
            && let Some(t) = self.next_reconnect
            && now >= t
        {
            self.reconnect_attempts += 1;
            self.push_line(
                Dir::Sys,
                format!("尝试重连 {}（第 {} 次）", port, self.reconnect_attempts),
            );
            // 打开失败会重置 next_reconnect，成功则清除重连目标
            self.open_port(&port, self.baud);
            if self.connected {
                self.reconnect_port = None;
                self.next_reconnect = None;
                self.reconnect_attempts = 0;
            } else {
                // 按 RECONNECT_INTERVAL 再次尝试
                self.next_reconnect = Some(Instant::now() + RECONNECT_INTERVAL);
            }
        }
    }

    fn send_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if !self.connected {
            self.push_line(Dir::Err, "未连接串口，发送失败");
            return;
        }
        let bytes = self.encoder.encode(text);
        if self.tx_sender.send(bytes).is_err() {
            self.push_line(Dir::Err, "发送通道已关闭（串口未连接）");
            return;
        }
        self.push_line(Dir::Tx, text.to_string());
        // 发送后自动回到最新输出，便于立即看到回显响应
        self.goto_bottom();
    }

    /// 当前输入的补全候选：无参数时补命令名，命令后补参数值（含动态串口名/日志文件）。
    /// 返回 (显示文本, 描述)，供补全面板与 Tab/↑↓ 共用。
    fn completion_candidates(&self) -> Vec<(String, String)> {
        let text = self.input_text();
        if !text.starts_with(':') {
            return Vec::new();
        }
        // 已输入命令名 + 空格，则补参数（按已输入参数前缀过滤）
        if let Some((cmd, arg_rest)) = text.split_once(' ') {
            let cmd = cmd.trim_end();
            // 已输入参数（含当前输入的部分）
            let arg_prefix = arg_rest.trim_start();
            // 回放命令：补应用目录下的 .log 文件（动态）
            if matches!(cmd, ":replay") {
                let files = replay_file_candidates(arg_prefix);
                if !files.is_empty() {
                    return files
                        .into_iter()
                        .map(|f| (f, "日志文件".to_string()))
                        .collect();
                }
            }
            // 串口类命令：补当前可用串口名（动态，走短 TTL 缓存避免连续按键反复枚举）
            if matches!(cmd, ":port" | ":connect" | ":reconnect") {
                let ports = cached_port_candidates();
                if !ports.is_empty() {
                    return ports
                        .iter()
                        .filter(|p| p.name().starts_with(arg_prefix))
                        .map(|p| (p.name().to_string(), "串口".to_string()))
                        .collect();
                }
            }
            if !param_values(cmd).is_empty() {
                let vals = param_values(cmd);
                return vals
                    .iter()
                    .filter(|v| v.starts_with(arg_prefix))
                    .map(|v| (v.to_string(), "参数".to_string()))
                    .collect();
            }
            // 已进入参数区（命令名后有空格）但参数候选为空：不再回退到命令名补全，
            // 否则会把命令名本身当作候选覆盖掉已输入参数（如 :replay stop 被改成 :replay :replay）
            return Vec::new();
        }
        // 补命令名（仅在命令名后无空格时）
        let cmd_part = text.split_once(' ').map(|(a, _)| a).unwrap_or(&text);
        let prefix = &cmd_part[1..];
        matching_commands(prefix)
            .into_iter()
            .map(|c| {
                let desc = COMMANDS
                    .iter()
                    .find(|(n, _)| *n == c)
                    .map(|(_, d)| *d)
                    .unwrap_or("");
                (c.to_string(), desc.to_string())
            })
            .collect()
    }

    /// Tab 命令/参数补全：多候选循环切换。已输入命令名时补参数值，否则补命令名。
    fn tab_complete(&mut self) {
        if !self.input_text().starts_with(':') {
            return;
        }
        let cands = self.completion_candidates();
        if cands.is_empty() {
            return;
        }
        // 前缀变化时重置选中索引
        self.ensure_completion_prefix();
        if self.completion_idx >= cands.len() {
            self.completion_idx = 0;
        }
        let chosen = cands[self.completion_idx].0.clone();
        self.completion_idx = (self.completion_idx + 1) % cands.len();
        self.apply_completion(&chosen);
    }

    /// 若输入文本相对上次补全时已变化，则重置选中索引到第一个候选
    fn ensure_completion_prefix(&mut self) {
        let text = self.input_text();
        if self.last_cmd_prefix != text {
            self.last_cmd_prefix = text;
            self.completion_idx = 0;
        }
    }

    /// ↑/↓ 在补全候选中移动高亮（只移动索引，不改写输入框，避免前缀变化导致无法继续切换）
    fn complete_move(&mut self, delta: i32) {
        if !self.input_text().starts_with(':') {
            return;
        }
        let cands = self.completion_candidates();
        if cands.is_empty() {
            return;
        }
        // 前缀变化时重置选中索引
        self.ensure_completion_prefix();
        let len = cands.len() as i32;
        self.completion_idx = ((self.completion_idx as i32 + delta + len) % len) as usize;
    }

    /// 若补全候选非空，将当前高亮的候选应用到输入框，返回是否应用
    fn complete_apply_selected(&mut self) -> bool {
        if !self.input_text().starts_with(':') {
            return false;
        }
        let cands = self.completion_candidates();
        if cands.is_empty() {
            return false;
        }
        if self.completion_idx >= cands.len() {
            self.completion_idx = 0;
        }
        let chosen = cands[self.completion_idx].0.clone();
        self.apply_completion(&chosen);
        true
    }

    /// 应用选中的补全项到输入框（光标移行尾）。
    /// 参数补全时替换/追加参数值，命令补全时替换命令名并加空格。
    fn apply_completion(&mut self, chosen: &str) {
        let text = self.input_text();
        // 判断是否为参数补全（已输入命令名 + 空格）
        let is_param = text
            .split_once(' ')
            .map(|(cmd, _)| {
                !param_values(cmd).is_empty()
                    || matches!(cmd, ":port" | ":connect" | ":reconnect" | ":replay")
            })
            .unwrap_or(false);
        if is_param {
            // 替换命令名后的参数部分
            let cmd_part = text.split_once(' ').map(|(c, _)| c).unwrap_or("");
            let new_text = format!("{} {}", cmd_part, chosen);
            self.set_input_text(new_text);
        } else {
            // 补命令名：若已有参数则只替换命令名并保留参数
            let rest = match text.find(' ') {
                Some(i) => text[i..].to_string(),
                None => String::new(),
            };
            let new_text = if rest.is_empty() {
                format!("{} ", chosen)
            } else {
                format!("{}{}", chosen, rest)
            };
            self.set_input_text(new_text);
        }
    }

    /// 鼠标事件：滚轮滚动历史；左键按下/拖动时暂停跟随（避免拖选时日志跳动）。
    fn handle_mouse(&mut self, m: crossterm::event::MouseEvent) {
        match m.kind {
            MouseEventKind::ScrollUp => self.scroll_up(3),
            MouseEventKind::ScrollDown => self.scroll_down(3),
            // 左键按下：暂停跟随，进入选择语义，让视口稳定便于选择
            MouseEventKind::Down(_) if self.follow => {
                self.scroll_up(0); // 切换为锚定模式，暂停跟随
            }
            // 左键按下但已在锚定模式：无操作
            MouseEventKind::Down(_) => {}
            MouseEventKind::Drag(_) => {
                self.follow = false;
            }
            _ => {}
        }
    }

    fn push_line(&mut self, dir: Dir, body: impl Into<String>) {
        let body = body.into();
        let ts = match self.ts_mode {
            TsMode::Off => None,
            TsMode::Short => Some(chrono::Local::now().format("%H:%M:%S%.3f").to_string()),
            TsMode::Iso => Some(chrono::Local::now().to_rfc3339()),
        };
        let line = LogLine { ts, dir, body };
        let formatted = self.format_line(&line);
        // 日志文件写入失败：关闭日志并提示一次，避免高频 RX 下重复刷屏
        let write_ok = self
            .log
            .as_mut()
            .is_none_or(|f| writeln!(f, "{}", formatted).is_ok());
        if !write_ok {
            self.log = None;
            eprintln!("日志写入失败，已停止记录");
        }
        self.lines.push_back(line);
        while self.lines.len() > 10_000 {
            self.lines.pop_front();
            // 关键：同步弹出对应的渲染缓存行。若不同步，lines 达到上限后
            // 长度不再变化但内容前移，draw 中 "render_rows.len() == lines.len()"
            // 的增量补齐判断会失效，导致新行永远不进入 render_rows，输出区卡住
            // 不显示新数据（RX 仍增长），只有 :clear 清空两者才能恢复。
            self.render_rows.pop_front();
        }
        self.dirty = true;
    }

    fn format_line(&self, l: &LogLine) -> String {
        let ts = l.ts.as_deref().unwrap_or("");
        let tag = match l.dir {
            Dir::Rx => "RX",
            Dir::Tx => "TX",
            Dir::Sys => "SYS",
            Dir::Err => "ERR",
        };
        format!("[{}] [{:<3}] {}", ts, tag, l.body)
    }

    // ---------- 交互 ----------

    /// 纯命令驱动：不设功能快捷键。
    /// 仅保留文本编辑、发送与命令输入必需的键位。
    fn handle_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);

        // 覆盖层优先（设置面板/帮助面板）
        if self.help_open {
            match k.code {
                KeyCode::Esc => self.help_open = false,
                KeyCode::Up | KeyCode::PageUp => {
                    self.help_scroll = self.help_scroll.saturating_sub(5);
                }
                KeyCode::Down | KeyCode::PageDown => self.help_scroll += 5,
                _ => {}
            }
            return;
        }
        if self.settings_open {
            self.handle_settings_key(k);
            return;
        }

        match k.code {
            KeyCode::Esc => {
                // 清空当前输入（编辑行为）
                if !self.textarea.is_empty() {
                    self.clear_input();
                }
            }
            KeyCode::Tab => self.tab_complete(),
            KeyCode::Enter if alt => {
                let _ = self.textarea.insert_str("\n");
            }
            KeyCode::Enter => {
                let content = self.input_text();
                if content.starts_with(':') {
                    // 补全候选可见时，先应用当前高亮项再执行，保证选中的命令生效
                    if !self.completion_candidates().is_empty() {
                        self.complete_apply_selected();
                    }
                    let cmd = self.input_text();
                    self.clear_input();
                    // 命令也进入发送历史，可按 ↑ 恢复上次使用的命令
                    self.history_add(&cmd);
                    self.execute_command(&cmd);
                } else if self.mode == SendMode::Direct {
                    if content.is_empty() {
                        self.send_text(&self.eol());
                    } else {
                        self.send_text(&content);
                        self.history_add(&content);
                        if !self.keep_input {
                            self.clear_input();
                        }
                    }
                } else {
                    if content.is_empty() {
                        return;
                    }
                    let with_eol = format!("{}{}", content, self.eol());
                    self.send_text(&with_eol);
                    self.history_add(&content);
                    if !self.keep_input {
                        self.clear_input();
                    }
                }
            }
            KeyCode::Up => {
                // 命令补全候选可见时，↑ 上移选择候选
                if !self.completion_candidates().is_empty() {
                    self.complete_move(-1);
                    return;
                }
                // 搜索已激活且输入框为空：↑ 跳转到上一个匹配
                if self.textarea.is_empty() && !self.search_matches.is_empty() {
                    self.search_jump(-1);
                    return;
                }
                if self.textarea.is_empty() && !self.history.is_empty() {
                    // hist_pos: 从最新往回数的位置（0=当前输入，1=最新一条）
                    if self.hist_pos == 0 {
                        self.hist_pos = 1;
                    } else {
                        self.hist_pos = (self.hist_pos + 1).min(self.history.len());
                    }
                    let idx = self.history.len() - self.hist_pos;
                    self.set_input_text(self.history[idx].clone());
                } else {
                    let _ = self.textarea.input(Input::from(k));
                }
            }
            KeyCode::Down => {
                // 命令补全候选可见时，↓ 下移选择候选
                if !self.completion_candidates().is_empty() {
                    self.complete_move(1);
                    return;
                }
                // 搜索已激活且输入框为空：↓ 跳转到下一个匹配
                if self.textarea.is_empty() && !self.search_matches.is_empty() {
                    self.search_jump(1);
                    return;
                }
                if self.hist_pos > 0 {
                    self.hist_pos -= 1;
                    if self.hist_pos == 0 {
                        self.clear_input();
                    } else {
                        let idx = self.history.len() - self.hist_pos;
                        self.set_input_text(self.history[idx].clone());
                    }
                } else if self.textarea.is_empty() {
                    self.clear_input();
                } else {
                    let _ = self.textarea.input(Input::from(k));
                }
            }
            _ => {
                // 即时模式：空输入框时逐字符直发
                if self.mode == SendMode::Direct
                    && self.textarea.is_empty()
                    && !ctrl
                    && !alt
                    && let KeyCode::Char(c) = k.code
                {
                    // 但 ':' 保留用于输入命令，不直发
                    if c == ':' {
                        let _ = self.textarea.insert_str(":");
                    } else {
                        self.send_text(&c.to_string());
                    }
                    return;
                }
                let _ = self.textarea.input(Input::from(k));
            }
        }
    }

    /// 渲染后的总行数（含折行）
    fn render_total(&self) -> usize {
        self.render_rows.iter().map(|r| r.len()).sum()
    }

    /// 上滑查看更早的历史（step 渲染行）。
    /// 首次上滑时从"跟随最新"切换为"锚定当前视口顶部"，
    /// 之后锚点随滚动移动，新数据到达不影响已锚定的显示位置。
    fn scroll_up(&mut self, step: usize) {
        if self.follow {
            // 从跟随切换为锚定：锚定在切换瞬间的视口顶部
            let total = self.render_total();
            let vis = self.out_height.max(1);
            self.view_top = total.saturating_sub(vis);
            self.follow = false;
        }
        self.view_top = self.view_top.saturating_sub(step);
    }

    /// 下滑查看较新的历史；回到底部时恢复跟随最新
    fn scroll_down(&mut self, step: usize) {
        if self.follow {
            return;
        }
        let total = self.render_total();
        let vis = self.out_height.max(1);
        let max_top = total.saturating_sub(vis);
        self.view_top = (self.view_top + step).min(max_top);
        if self.view_top >= max_top {
            self.follow = true;
            self.view_top = 0;
        }
    }

    fn eol(&self) -> String {
        match self.tx_on_enter.as_str() {
            "CR" => "\r".into(),
            "LF" => "\n".into(),
            _ => "\r\n".into(),
        }
    }

    fn toggle_hex(&mut self) {
        if self.enc == Encoding::Raw {
            // 从 Hex 切回文本：恢复进入 Hex 前的编码
            self.enc = self.pre_hex_enc;
        } else {
            self.pre_hex_enc = self.enc;
            self.enc = Encoding::Raw;
        }
        self.reset_codecs();
        self.push_line(Dir::Sys, format!("显示模式: {}", self.enc.label()));
    }

    fn mode_label(&self) -> &'static str {
        match self.mode {
            SendMode::Line => "行缓冲",
            SendMode::Direct => "即时",
        }
    }

    fn refresh_port_candidates(&mut self) {
        self.port_candidates = list_ports();
        if self.port_idx >= self.port_candidates.len() && !self.port_candidates.is_empty() {
            self.port_idx = 0;
        }
    }

    /// 设置面板按键处理
    fn handle_settings_key(&mut self, k: KeyEvent) {
        const N_FIELDS: usize = 8; // 端口/波特率/数据位/停止位/校验位/流控/编码/发送模式
        const N_ROWS: usize = N_FIELDS + 2; // + 连接/取消按钮
        match k.code {
            KeyCode::Up => self.settings_sel = self.settings_sel.saturating_sub(1),
            KeyCode::Down if self.settings_sel + 1 < N_ROWS => self.settings_sel += 1,
            KeyCode::Down => {}
            KeyCode::Tab => self.settings_sel = (self.settings_sel + 1) % N_ROWS,
            KeyCode::BackTab => {
                self.settings_sel = (self.settings_sel + N_ROWS - 1) % N_ROWS;
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') => {
                let fwd = k.code == KeyCode::Right || k.code == KeyCode::Char(' ');
                self.cycle_setting(fwd);
            }
            KeyCode::Enter => {
                if self.settings_sel == N_FIELDS {
                    // [连接]
                    self.apply_settings();
                    self.settings_open = false;
                } else if self.settings_sel == N_FIELDS + 1 {
                    // [取消]
                    self.settings_open = false;
                } else {
                    // 普通字段：Enter 等价于右移切换
                    self.cycle_setting(true);
                }
            }
            KeyCode::Esc => self.settings_open = false,
            _ => {}
        }
    }

    /// 循环切换当前字段的值
    fn cycle_setting(&mut self, forward: bool) {
        let delta = if forward { 1 } else { -1 };
        match self.settings_sel {
            0 => {
                if self.port_candidates.is_empty() {
                    // 快捷键已移除，改用命令提示
                    self.push_line(Dir::Err, "无可用端口，请连接设备后输入 :settings 刷新");
                    return;
                }
                let len = self.port_candidates.len() as i32;
                self.port_idx = ((self.port_idx as i32 + delta + len) % len) as usize;
            }
            1 => self.baud = cycle_value(&BAUDS, self.baud, delta),
            2 => self.databits = cycle_value(&DATA_BITS, self.databits, delta),
            3 => self.stopbits = cycle_str(&STOP_BITS, &self.stopbits, delta).to_string(),
            4 => self.parity = cycle_str(&PARITIES, &self.parity, delta).to_string(),
            5 => self.flow = cycle_str(&FLOWS, &self.flow, delta).to_string(),
            6 => {
                let arr = [Encoding::Utf8, Encoding::Gbk, Encoding::Auto, Encoding::Raw];
                let i = arr.iter().position(|&x| x == self.enc).unwrap_or(0);
                let len = arr.len() as i32;
                self.enc = arr[((i as i32 + delta + len) % len) as usize];
            }
            7 => {
                self.mode = if self.mode == SendMode::Line {
                    SendMode::Direct
                } else {
                    SendMode::Line
                };
            }
            _ => {}
        }
    }

    /// 应用设置并连接
    fn apply_settings(&mut self) {
        let port = self
            .port_candidates
            .get(self.port_idx)
            .map(|p| p.name().to_string())
            .unwrap_or_default();
        if port.is_empty() {
            self.push_line(Dir::Err, "未选择端口");
            return;
        }
        self.reset_codecs();
        self.open_port(&port, self.baud);
        self.push_line(
            Dir::Sys,
            format!(
                "已应用: {} @ {} baud {}-{}-{} 编码:{} 模式:{}",
                port,
                self.baud,
                self.databits,
                self.stopbits,
                self.parity,
                self.enc.label(),
                self.mode_label()
            ),
        );
    }

    /// 当前输入框的完整文本（含多行）
    fn input_text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    fn clear_input(&mut self) {
        self.textarea.select_all();
        self.textarea.input(tui_textarea::Input {
            key: tui_textarea::Key::Backspace,
            ctrl: false,
            alt: false,
            shift: false,
        });
    }

    /// 整体设置输入框内容并把光标移到行尾（恢复历史/补全后光标不应停留在行首）
    fn set_input_text(&mut self, text: String) {
        let mut ta = TextArea::from(vec![text]);
        ta.move_cursor(tui_textarea::CursorMove::End);
        self.textarea = ta;
    }

    fn history_add(&mut self, text: &str) {
        let t = text.to_string();
        if self.history.last() == Some(&t) {
            // 与最新一条相同则不重复入队，但历史浏览位置仍应复位
            self.hist_pos = 0;
            return;
        }
        self.history.push(t);
        while self.history.len() > 200 {
            self.history.remove(0);
        }
        self.hist_pos = 0;
    }

    fn execute_command(&mut self, cmd: &str) {
        let cmd = cmd.trim();
        let (name, arg) = cmd
            .split_once(' ')
            .map(|(a, b)| (a, b.trim()))
            .unwrap_or((cmd, ""));
        // 滚动类命令执行后保持当前视口；其余命令（含发送/参数调整）执行后回到最新输出
        // 搜索也属滚动类：定位到匹配行后不应跳回底部（:search 与 :find 均需排除）
        let is_scroll_cmd = matches!(
            name,
            ":scroll"
                | ":up"
                | ":pgup"
                | ":down"
                | ":pgdn"
                | ":top"
                | ":tail"
                | ":live"
                | ":follow"
                | ":search"
                | ":find"
        );
        match name {
            // ---- 帮助与退出 ----
            ":help" | "?" => {
                self.help_scroll = 0;
                self.help_open = true;
            }
            ":quit" | ":exit" => self.quit = true,
            ":clear" => {
                // 同时清空折行渲染缓存，避免依赖 draw 逐帧 pop_front 对齐
                self.lines.clear();
                self.render_rows.clear();
                self.follow = true;
                self.view_top = 0;
                // 清空搜索状态
                self.search_kw = None;
                self.search_matches.clear();
                self.push_line(Dir::Sys, "已清屏");
            }

            // ---- 连接管理 ----
            ":settings" | ":config" => {
                self.refresh_port_candidates();
                self.settings_open = true;
                self.settings_sel = 0;
            }
            ":list" => {
                let ports = list_ports();
                if ports.is_empty() {
                    self.push_line(Dir::Err, "未检测到可用串口");
                } else {
                    for p in &ports {
                        let info = p
                            .summary()
                            .map(|s| format!("  [{}]", s))
                            .unwrap_or_default();
                        self.push_line(Dir::Sys, format!("{}{}", p.name(), info));
                    }
                    self.push_line(Dir::Sys, format!("共 {} 个串口", ports.len()));
                }
            }
            ":status" => {
                let state = if self.connected {
                    "已连接"
                } else {
                    "未连接"
                };
                let port = self.port_name.clone().unwrap_or_else(|| "--".into());
                self.push_line(
                    Dir::Sys,
                    format!(
                        "状态:{} {} @ {} baud {}-{}-{} 编码:{} 模式:{} RX:{} TX:{}",
                        state,
                        port,
                        self.baud,
                        self.databits,
                        self.stopbits,
                        self.parity,
                        self.enc.label(),
                        self.mode_label(),
                        self.rx_bytes,
                        self.tx_bytes
                    ),
                );
            }
            ":port" | ":connect" => {
                let name = if let Ok(n) = arg.parse::<u32>() {
                    format!("COM{}", n)
                } else {
                    arg.to_string()
                };
                if name.is_empty() {
                    self.push_line(Dir::Err, "用法: :port COM4 或 :port 3");
                    self.goto_bottom();
                    return;
                }
                self.open_port(&name, self.baud);
            }
            ":close" | ":disconnect" => {
                self.stop_threads();
                self.connected = false;
                self.port_name = None;
                // 手动关闭时停止自动重连，避免误判为意外断开而反复重连
                self.reconnect_port = None;
                self.next_reconnect = None;
                self.push_line(Dir::Sys, "串口已关闭");
            }

            // ---- 参数调整（已连接时自动重连生效） ----
            ":baud" => {
                if let Ok(n) = arg.parse::<u32>() {
                    self.baud = n;
                    self.push_line(Dir::Sys, format!("波特率: {}", n));
                    self.reconnect_current();
                } else {
                    self.push_line(Dir::Err, format!("波特率无效: {}", arg));
                }
            }
            ":databits" => {
                if let Ok(n) = arg.parse::<u8>()
                    && (5..=8).contains(&n)
                {
                    self.databits = n;
                    self.push_line(Dir::Sys, format!("数据位: {}", n));
                    self.reconnect_current();
                } else {
                    self.push_line(Dir::Err, "数据位需为 5-8");
                }
            }
            ":stopbits" => {
                if matches!(arg, "1" | "2") {
                    self.stopbits = arg.to_string();
                    self.push_line(Dir::Sys, format!("停止位: {}", arg));
                    self.reconnect_current();
                } else {
                    self.push_line(Dir::Err, "停止位需为 1 或 2");
                }
            }
            ":parity" => {
                if matches!(arg, "none" | "odd" | "even") {
                    self.parity = arg.to_string();
                    self.push_line(Dir::Sys, format!("校验位: {}", arg));
                    self.reconnect_current();
                } else {
                    self.push_line(Dir::Err, "校验位需为 none/odd/even");
                }
            }
            ":flow" => {
                if matches!(arg, "none" | "software" | "hardware") {
                    self.flow = arg.to_string();
                    self.push_line(Dir::Sys, format!("流控: {}", arg));
                    self.reconnect_current();
                } else {
                    self.push_line(Dir::Err, "流控需为 none/software/hardware");
                }
            }
            ":encoding" => {
                if arg.is_empty() {
                    self.push_line(Dir::Err, "用法: :encoding utf8|gbk|auto|raw");
                } else {
                    self.enc = Encoding::from_str_name(arg);
                    self.reset_codecs();
                    self.push_line(Dir::Sys, format!("编码: {}", self.enc.label()));
                }
            }
            ":mode" => match arg {
                "line" => {
                    self.mode = SendMode::Line;
                    self.push_line(Dir::Sys, "发送模式: 行缓冲");
                }
                "direct" => {
                    self.mode = SendMode::Direct;
                    self.push_line(Dir::Sys, "发送模式: 即时");
                }
                _ => self.push_line(Dir::Err, "用法: :mode line|direct"),
            },
            ":theme" => {
                if arg.is_empty() {
                    self.push_line(
                        Dir::Sys,
                        format!(
                            "当前主题: {}（可用: {}）",
                            self.theme.name_of(),
                            Theme::NAMES.join(", ")
                        ),
                    );
                } else if Theme::NAMES.contains(&arg) {
                    self.theme = Theme::from_name(arg);
                    self.dirty = true;
                    self.push_line(Dir::Sys, format!("已切换主题: {}", arg));
                } else {
                    self.push_line(
                        Dir::Err,
                        format!("未知主题: {}（可用: {}）", arg, Theme::NAMES.join(", ")),
                    );
                }
            }

            // ---- 显示 ----
            ":hex" => {
                if arg.is_empty() {
                    // 无参数：切换 Hex 视图
                    self.toggle_hex();
                } else if let Some(bytes) = parse_hex(arg) {
                    // 有参数：十六进制发送
                    if !self.connected {
                        self.push_line(Dir::Err, "未连接串口，发送失败");
                    } else if self.tx_sender.send(bytes).is_err() {
                        self.push_line(Dir::Err, "发送通道已关闭");
                    } else {
                        self.push_line(Dir::Tx, format!("(HEX) {}", arg));
                    }
                } else {
                    self.push_line(Dir::Err, "十六进制格式无效，如 :hex 41 42 43");
                }
            }
            ":text" => {
                self.enc = Encoding::Utf8;
                self.reset_codecs();
                self.push_line(Dir::Sys, "显示模式: 文本");
            }
            ":ts" => match arg {
                "on" | "short" => {
                    self.ts_mode = TsMode::Short;
                    self.push_line(Dir::Sys, "时间戳: 开启");
                }
                "off" => {
                    self.ts_mode = TsMode::Off;
                    self.push_line(Dir::Sys, "时间戳: 关闭");
                }
                "iso" => {
                    self.ts_mode = TsMode::Iso;
                    self.push_line(Dir::Sys, "时间戳: ISO");
                }
                _ => self.push_line(Dir::Err, "用法: :ts on|off|iso"),
            },
            ":tail" | ":live" | ":follow" => {
                self.follow = true;
                self.view_top = 0;
                self.push_line(Dir::Sys, "已回到最新输出");
            }
            ":top" => {
                // 一键回到输出最顶部（最早记录），进入锚定模式
                self.follow = false;
                self.view_top = 0;
                self.push_line(Dir::Sys, "已回到输出顶部");
            }
            ":search" | ":find" => {
                self.search_output(arg);
            }
            ":up" | ":pgup" => self.scroll_up(10),
            ":down" | ":pgdn" => self.scroll_down(10),
            ":scroll" => match arg {
                "up" | "u" => self.scroll_up(10),
                "down" | "d" => self.scroll_down(10),
                n => {
                    if let Ok(rows) = n.parse::<usize>() {
                        self.scroll_up(rows);
                    } else {
                        self.push_line(Dir::Err, "用法: :scroll up|down|<行数>");
                    }
                }
            },

            // ---- 发送 ----
            ":send" => {
                let text = arg
                    .trim_matches('"')
                    .replace("\\r", "\r")
                    .replace("\\n", "\n")
                    .replace("\\t", "\t");
                self.send_text(&text);
            }

            // ---- 日志 ----
            ":log" => match arg {
                "on" => {
                    let name = format!(
                        "serial_{}.log",
                        chrono::Local::now().format("%Y%m%d_%H%M%S")
                    );
                    let path = resolve_output_path(&name);
                    match File::create(&path) {
                        Ok(f) => {
                            self.push_line(Dir::Sys, format!("日志已开启 -> {}", path.display()));
                            self.log = Some(BufWriter::new(f));
                        }
                        Err(e) => self.push_line(Dir::Err, format!("创建日志失败: {}", e)),
                    }
                }
                "off" => {
                    self.log = None;
                    self.push_line(Dir::Sys, "日志已关闭");
                }
                _ => self.push_line(Dir::Err, "用法: :log on|off"),
            },

            // ---- 最近设备与断线重连 ----
            ":recent" => {
                if let Ok(idx) = arg.parse::<usize>() {
                    // 带序号：连接对应历史设备（恢复其参数）
                    // [借用安全] clone 出所需数据，避免持有 recent 的借用期间调用 &mut self
                    let (port, baud, databits, stopbits, parity, flow, encoding) =
                        match self.cfg.cfg.recent.get(idx) {
                            Some(r) => (
                                r.port.clone(),
                                r.baud,
                                r.databits,
                                r.stopbits.clone(),
                                r.parity.clone(),
                                r.flow.clone(),
                                r.encoding.clone(),
                            ),
                            None => {
                                self.push_line(Dir::Err, format!("序号 {} 超出范围", idx));
                                self.goto_bottom();
                                return;
                            }
                        };
                    self.databits = databits;
                    self.stopbits = stopbits;
                    self.parity = parity;
                    self.flow = flow;
                    let enc = Encoding::from_str_name(&encoding);
                    if enc != self.enc {
                        self.enc = enc;
                        self.reset_codecs();
                    }
                    self.push_line(
                        Dir::Sys,
                        format!(
                            "连接历史设备 {} @ {} ({}-{}-{})",
                            port, baud, self.databits, self.stopbits, self.parity
                        ),
                    );
                    self.open_port(&port, baud);
                    return;
                }
                if self.cfg.cfg.recent.is_empty() {
                    self.push_line(Dir::Sys, "暂无历史设备记录");
                } else {
                    let recent_snapshot: Vec<RecentSnapshot> = self
                        .cfg
                        .cfg
                        .recent
                        .iter()
                        .enumerate()
                        .map(|(i, r)| {
                            (
                                i,
                                r.port.clone(),
                                r.baud,
                                r.databits,
                                r.stopbits.clone(),
                                r.parity.clone(),
                                r.encoding.clone(),
                                r.conn_count,
                                self.port_name.as_deref() == Some(r.port.as_str()),
                            )
                        })
                        .collect();
                    for (i, port, baud, db, sb, parity, encoding, cnt, cur) in recent_snapshot {
                        let cur_s = if cur { " ●当前" } else { "" };
                        self.push_line(
                            Dir::Sys,
                            format!(
                                "[{}] {} @ {} {}-{}-{} 编码:{} 连接{}次{}",
                                i, port, baud, db, sb, parity, encoding, cnt, cur_s
                            ),
                        );
                    }
                    self.push_line(Dir::Sys, "用法: :recent <序号> 连接对应设备");
                }
            }
            ":reconnect" | ":auto" => {
                if arg == "off" {
                    self.reconnect_port = None;
                    self.next_reconnect = None;
                    self.push_line(Dir::Sys, "已关闭断线自动重连");
                    self.goto_bottom();
                    return;
                }
                if arg.is_empty() {
                    // 无参数：显示当前重连状态
                    let state = self.reconnect_port.clone().unwrap_or_else(|| "关闭".into());
                    self.push_line(Dir::Sys, format!("断线自动重连: {}", state));
                    self.goto_bottom();
                    return;
                }
                let port = normalize_port(arg);
                self.reconnect_port = Some(port.clone());
                self.next_reconnect = None;
                self.reconnect_attempts = 0;
                self.push_line(Dir::Sys, format!("已启用断线自动重连 -> {}", port));
                // 立即尝试一次重连，无需等待下一轮 tick
                if !self.connected {
                    self.open_port(&port, self.baud);
                    if !self.connected {
                        self.next_reconnect = Some(Instant::now() + RECONNECT_INTERVAL);
                    }
                }
            }

            // ---- 日志导出与回放 ----
            ":export" => {
                if let Some(path) = self.export_logs(arg) {
                    self.push_line(Dir::Sys, format!("日志已导出 -> {}", path));
                }
            }
            ":replay" => {
                if arg == "stop" {
                    self.stop_replay();
                } else {
                    // 解析 "文件 [倍率]"，倍率为可选数字
                    let mut parts = arg.split_whitespace();
                    let file = parts.next().unwrap_or("");
                    let speed = parts
                        .next()
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(1.0);
                    self.start_replay(file, speed);
                }
            }
            "" => {}
            _ => self.push_line(Dir::Sys, format!("未知命令: {}（:help 查看）", name)),
        }
        // 非滚动类命令执行后自动回到最新输出（便于查看命令执行结果/回显）
        if !is_scroll_cmd {
            self.goto_bottom();
        }
    }

    /// 回到最新输出（跟随模式）
    fn goto_bottom(&mut self) {
        self.follow = true;
        self.view_top = 0;
    }

    /// 在输出区历史中搜索包含关键词的行，并跳到指定匹配位置。
    /// 用法：`:search <关键词> [序号]`，序号为第 N 个匹配（从 1 开始），默认跳到第一个。
    fn search_output(&mut self, arg: &str) {
        // 无参数（或 off/clear）：清除当前搜索高亮
        if arg.trim().is_empty() || matches!(arg.trim(), "off" | "clear" | "none") {
            let had = self.search_kw.is_some();
            self.search_kw = None;
            self.search_matches.clear();
            self.search_pos = 0;
            if had {
                self.render_w = usize::MAX; // 清除高亮后重建缓存
                self.push_line(Dir::Sys, "已清除搜索高亮");
            } else {
                self.push_line(Dir::Sys, "当前没有搜索高亮");
            }
            self.goto_bottom();
            return;
        }
        if self.lines.is_empty() {
            self.push_line(Dir::Sys, "当前没有可搜索的日志");
            self.goto_bottom();
            return;
        }
        // 解析尾部可选序号（最后一个空格分隔的词若是纯数字则视为序号）
        let parts: Vec<&str> = arg.split_whitespace().collect();
        let (keyword, target_idx) = if parts.len() >= 2
            && let Ok(n) = parts.last().unwrap().parse::<usize>()
        {
            let kw = parts[..parts.len() - 1].join(" ");
            (kw, n.saturating_sub(1))
        } else {
            (arg.trim().to_string(), 0)
        };
        // 记录匹配的（日志行索引）列表
        let kw = keyword.to_lowercase();
        let matches: Vec<usize> = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.body.to_lowercase().contains(&kw))
            .map(|(i, _)| i)
            .collect();
        if matches.is_empty() {
            self.push_line(Dir::Sys, format!("未找到包含 \"{}\" 的内容", keyword));
            self.search_kw = None; // 无匹配时清除高亮
            self.search_matches.clear();
            self.render_w = usize::MAX; // 清除高亮后同样需重建缓存
            self.goto_bottom();
            return;
        }
        // 序号超出范围时提示可用的范围
        if target_idx >= matches.len() {
            self.push_line(
                Dir::Err,
                format!(
                    "序号 {} 超出范围（共 {} 条匹配）",
                    target_idx + 1,
                    matches.len()
                ),
            );
            self.goto_bottom();
            return;
        }
        // 记录搜索关键词与匹配列表（供高亮与 ↑↓ 跳转）
        self.search_kw = Some(keyword.to_lowercase());
        self.search_matches = matches.clone();
        self.search_pos = target_idx;
        // 关键词变化会改变已有行的渲染（高亮），必须使折行缓存失效以便重建
        self.render_w = usize::MAX; // 与任何 inner_w 都不匹配，强制 draw 时整体重建
        // 跳到第 N 个匹配行的顶部（锚定模式）
        let match_line = matches[target_idx];
        self.follow = false;
        self.view_top = self.render_top_of(match_line);
        self.push_line(
            Dir::Sys,
            format!(
                "找到 {} 条包含 \"{}\"，已定位到第 {} / {} 条（↑↓ 切换）",
                matches.len(),
                keyword,
                target_idx + 1,
                matches.len()
            ),
        );
    }

    /// 计算某条日志行的渲染起始行（用于搜索定位后锚定）
    fn render_top_of(&self, line_idx: usize) -> usize {
        self.render_rows
            .iter()
            .take(line_idx)
            .map(|r| r.len())
            .sum()
    }

    /// 在搜索结果之间跳转（↑/↓ 时由 handle_key 调用）
    fn search_jump(&mut self, delta: i32) {
        if self.search_matches.is_empty() {
            return;
        }
        let len = self.search_matches.len() as i32;
        // 循环跳转
        self.search_pos = ((self.search_pos as i32 + delta + len) % len) as usize;
        let match_line = self.search_matches[self.search_pos];
        self.follow = false;
        self.view_top = self.render_top_of(match_line);
        self.push_line(
            Dir::Sys,
            format!(
                "已定位到第 {} / {} 条",
                self.search_pos + 1,
                self.search_matches.len()
            ),
        );
    }

    /// 参数变更后重连当前端口（未连接则仅更新参数，供下次连接生效）
    fn reconnect_current(&mut self) {
        if self.connected
            && let Some(p) = self.port_name.clone()
        {
            self.open_port(&p, self.baud);
        }
    }

    // ---------- 渲染 ----------

    fn status_text(&self) -> String {
        let dot = if self.connected { "●" } else { "○" };
        let port = self.port_name.as_deref().unwrap_or("--");
        let parity = match self.parity.as_str() {
            "odd" => "O",
            "even" => "E",
            "mark" => "M",
            "space" => "S",
            _ => "N",
        };
        format!(
            " {} {} │ {} {}-{}-{} │ 编码:{} │ {} │ RX {} │ TX {} │ :help",
            dot,
            port,
            self.baud,
            self.databits,
            parity,
            self.stopbits,
            self.enc.label(),
            self.mode_label(),
            self.rx_bytes,
            self.tx_bytes
        )
    }

    /// 日志行前缀（时间戳 + RX/TX 标签），返回 spans 及其显示宽度
    fn log_prefix(&self, l: &LogLine) -> (Vec<Span<'static>>, usize) {
        let theme = &self.theme;
        let mut spans: Vec<Span> = Vec::new();
        if let Some(ts) = &l.ts {
            spans.push(Span::styled(
                format!("[{}] ", ts),
                Style::new().fg(Color::DarkGray),
            ));
        }
        let (tag, color) = match l.dir {
            Dir::Rx => ("RX", theme.rx),
            Dir::Tx => ("TX", theme.tx),
            Dir::Sys => ("SYS", theme.sys),
            Dir::Err => ("ERR", theme.err),
        };
        if !self.no_tags {
            spans.push(Span::styled(
                format!("[{:<3}] ", tag),
                Style::new().fg(color),
            ));
        }
        let w = spans.iter().map(|s| s.content.width()).sum::<usize>();
        (spans, w)
    }

    /// 将一条日志按终端宽度折成多行渲染行（续行与前缀对齐缩进，保留 ANSI 颜色）
    fn wrap_log_line(&self, l: &LogLine, width: usize) -> Vec<Line<'static>> {
        let (prefix, prefix_w) = self.log_prefix(l);
        let body_style = Style::new().fg(self.theme.fg);
        let mut body_spans = ansi::ansi_to_spans(&l.body, body_style);
        // 搜索高亮：若当前有搜索关键词且本行命中（基于去 ANSI 的纯文本），对命中片段应用高亮样式
        if let Some(kw) = &self.search_kw {
            let plain: String = body_spans.iter().map(|s| s.content.as_ref()).collect();
            if plain.to_lowercase().contains(kw) {
                let hl = Style::new().bg(self.theme.accent).fg(Color::Black);
                body_spans = highlight_keyword(body_spans, kw, hl);
            }
        }
        let body_w = width.saturating_sub(prefix_w);
        let chunks = wrap_spans(&body_spans, body_w);
        let mut rows = Vec::with_capacity(chunks.len());
        for (i, chunk) in chunks.into_iter().enumerate() {
            let mut spans: Vec<Span> = Vec::new();
            if i == 0 {
                spans.extend(prefix.iter().cloned());
            } else {
                // 续行缩进对齐到正文首列
                spans.push(Span::styled(" ".repeat(prefix_w), Style::new()));
            }
            spans.extend(chunk);
            rows.push(Line::from(spans));
        }
        if rows.is_empty() {
            rows.push(Line::from(prefix));
        }
        rows
    }

    fn input_title(&self) -> String {
        let mode = if self.mode == SendMode::Line {
            "行发送"
        } else {
            "即时"
        };
        format!("> 输入(:命令)  [{}·{}]", mode, self.enc.label())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let ta_h = (self.textarea.lines().len() + 2).clamp(3, 7) as u16;

        // 命令补全候选（输入框以 ":" 开头时实时显示）
        let cands = self.completion_candidates();
        let cand_vis = cands.len().min(6);
        let has_cands = cand_vis > 0;

        let mut constraints = vec![Constraint::Length(1), Constraint::Min(1)];
        if has_cands {
            constraints.push(Constraint::Length(cand_vis as u16 + 2));
        }
        constraints.push(Constraint::Length(ta_h));
        let chunks = Layout::vertical(constraints).split(area);
        let input_idx = if has_cands { 3 } else { 2 };

        let theme = &self.theme;

        // 状态栏
        let status =
            Paragraph::new(self.status_text()).style(Style::new().fg(theme.accent).bg(theme.bg));
        frame.render_widget(status, chunks[0]);

        // 输出区：长行按屏幕宽度自动折行（自适应显示），滚动基于渲染行。
        // follow=true 显示最新一屏；查看历史时锚定 view_top（渲染行索引），
        // 新数据到达不影响当前显示位置。
        let out_area = chunks[1];
        // 边框上下各占 1 行，内部可视行数与可用宽度据此扣除
        let inner_w = out_area.width.saturating_sub(2) as usize;
        let inner_vis = out_area.height.saturating_sub(2) as usize;
        self.out_height = inner_vis;

        // 同步折行缓存：宽度变化整体重建，否则增量补齐新增日志行
        if self.render_w != inner_w {
            self.render_w = inner_w;
            self.render_rows.clear();
            for l in &self.lines {
                self.render_rows.push_back(self.wrap_log_line(l, inner_w));
            }
        } else {
            while self.render_rows.len() < self.lines.len() {
                let l = &self.lines[self.render_rows.len()];
                self.render_rows.push_back(self.wrap_log_line(l, inner_w));
            }
            while self.render_rows.len() > self.lines.len() {
                self.render_rows.pop_front();
            }
        }

        // 计算视口（渲染行索引）
        let total = self.render_total();
        let start = if self.follow {
            total.saturating_sub(inner_vis)
        } else {
            self.view_top.min(total.saturating_sub(inner_vis))
        };
        let end = (start + inner_vis).min(total);

        // 收集 [start, end) 范围内的渲染行
        let mut view: Vec<Line> = Vec::with_capacity(inner_vis);
        let mut acc = 0usize;
        for rows in &self.render_rows {
            let n = rows.len();
            if acc + n <= start {
                acc += n;
                continue;
            }
            for (i, line) in rows.iter().enumerate() {
                let idx = acc + i;
                if idx >= start && idx < end {
                    view.push(line.clone());
                }
            }
            acc += n;
            if acc >= end {
                break;
            }
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::new().fg(theme.border));
        frame.render_widget(Paragraph::new(view).block(block), out_area);

        // 输入区
        let title = self.input_title();
        self.textarea.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {}", title))
                .title_style(Style::new().fg(theme.accent))
                .border_style(Style::new().fg(theme.border)),
        );
        self.textarea
            .set_cursor_style(Style::new().fg(theme.accent));
        frame.render_widget(&self.textarea, chunks[input_idx]);

        // 命令补全面板（输入框以 ":" 开头时显示，Tab/↑↓ 循环切换）
        // 选中项可能超出可视窗口：计算窗口起点使高亮始终可见，可切换全部候选
        if has_cands {
            let sel = self.completion_idx.min(cands.len().saturating_sub(1));
            let vis_start = if sel >= cand_vis {
                sel + 1 - cand_vis
            } else {
                0
            };
            self.render_completion(frame, &cands, vis_start, cand_vis, chunks[2]);
        }

        // 覆盖层
        if self.help_open {
            self.render_help(frame);
        }
        if self.settings_open {
            self.render_settings(frame);
        }
    }

    /// 命令补全面板：实时展示匹配当前输入的候选命令，当前 Tab 选中项高亮
    /// 渲染补全菜单。`cands` 为完整候选，`vis_start` 为可视窗口起点，
    /// `vis_len` 为可视行数。选中项（completion_idx）通过滚动保证始终可见，
    /// 使超过可视高度的候选也能通过 Tab/↑↓ 切换到。
    fn render_completion(
        &self,
        frame: &mut Frame,
        cands: &[(String, String)],
        vis_start: usize,
        vis_len: usize,
        area: Rect,
    ) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" 补全 (Tab/↑↓切换) ")
            .title_style(Style::new().fg(self.theme.accent))
            .border_style(Style::new().fg(self.theme.border))
            .bg(self.theme.bg);
        let mut items: Vec<Line> = Vec::with_capacity(vis_len);
        for i in 0..vis_len {
            let idx = vis_start + i;
            if idx >= cands.len() {
                break;
            }
            let (c, desc) = &cands[idx];
            let selected = idx == self.completion_idx;
            if selected {
                items.push(Line::from(vec![Span::styled(
                    format!("▸ {:<13} {}", c, desc),
                    Style::new()
                        .fg(Color::Black)
                        .bg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                )]));
            } else {
                items.push(Line::from(vec![Span::styled(
                    format!("  {:<13} {}", c, desc),
                    Style::new().fg(self.theme.fg),
                )]));
            }
        }
        let p = Paragraph::new(items).block(block);
        frame.render_widget(p, area);
    }

    /// 帮助面板：按分组动态渲染，命令名/描述分色，支持 ↑↓ 滚动
    fn render_help(&mut self, frame: &mut Frame) {
        let theme = &self.theme;
        let area = centered_rect(68, 90, frame.area());
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" 📖 帮助 (↑↓滚动 Esc关闭) ")
            .title_style(Style::new().fg(theme.accent).add_modifier(Modifier::BOLD))
            .border_style(Style::new().fg(theme.border))
            .bg(theme.bg);
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);

        let mut lines: Vec<Line> = Vec::new();
        for (group, cmds) in HELP_GROUPS {
            lines.push(Line::from(vec![Span::styled(
                format!("▍{}", group),
                Style::new().fg(theme.sys).add_modifier(Modifier::BOLD),
            )]));
            for c in *cmds {
                let desc = COMMANDS
                    .iter()
                    .find(|(n, _)| n == c)
                    .map(|(_, d)| *d)
                    .unwrap_or("");
                lines.push(Line::from(vec![
                    Span::styled(format!("  {:<13}", c), Style::new().fg(theme.accent)),
                    Span::styled(desc, Style::new().fg(theme.fg)),
                ]));
            }
            lines.push(Line::from(" "));
        }
        lines.push(Line::from(Span::styled(
            "  普通输入按 Enter 直接发送；Tab 补全；↑↓选择候选；滚轮/PageUp 浏览历史；Shift+拖选复制文本",
            Style::new().fg(Color::DarkGray),
        )));

        // 内容超高时允许滚动（防止 scroll 越界）
        let total = lines.len();
        let inner_h = inner.height as usize;
        if total > inner_h {
            self.help_scroll = self.help_scroll.min(total - inner_h);
        }
        let p = Paragraph::new(lines).scroll((self.help_scroll as u16, 0));
        frame.render_widget(p, inner);
    }

    /// 设置面板（串口配置弹窗）—— 分组标题 + 按钮行 + 操作提示，整体居中显示
    fn render_settings(&self, frame: &mut Frame) {
        let theme = &self.theme;
        // 面板宽度贴合参数内容，整体居中；面板内部参数行左对齐
        let area = centered_rect(34, 78, frame.area());
        let outer = Block::default()
            .borders(Borders::ALL)
            .title(" ⚙ 串口设置 ")
            .title_style(Style::new().fg(theme.accent).add_modifier(Modifier::BOLD))
            .border_style(Style::new().fg(theme.border))
            .bg(theme.bg);
        let inner = outer.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(outer, area);

        // 面板内垂直布局：标题 + 参数(8) + 分隔 + 按钮 + 提示 + 设备信息
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(8),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(2),
        ])
        .split(inner);

        // 分组标题（左对齐）
        let group = Line::from(vec![
            Span::styled(
                "▍连接参数",
                Style::new().fg(theme.sys).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::new()),
        ]);
        frame.render_widget(Paragraph::new(group), chunks[0]);

        // 参数行：每行等宽（标签 + 值 + 填充），整体居中时标签列与值列分别对齐
        let port_count = self.port_candidates.len();
        let port_display = match self.port_candidates.get(self.port_idx) {
            Some(p) if port_count > 0 => format!("{}（共{}）", p.name(), port_count),
            _ => "无可用端口".to_string(),
        };
        // 内容宽度（面板内宽），每行都补齐到该宽度保证等宽
        let content_w = (inner.width.saturating_sub(4)) as usize;
        let param_rows: Vec<Line> = vec![
            self.settings_row(0, "端口", &port_display, content_w),
            self.settings_row(1, "波特率", &self.baud.to_string(), content_w),
            self.settings_row(2, "数据位", &self.databits.to_string(), content_w),
            self.settings_row(3, "停止位", &self.stopbits, content_w),
            self.settings_row(4, "校验位", &self.parity, content_w),
            self.settings_row(5, "流控", &self.flow, content_w),
            self.settings_row(6, "编码", self.enc.label(), content_w),
            self.settings_row(7, "发送模式", self.mode_label(), content_w),
        ];
        // 参数行整体居中；因每行等宽，居中后标签列与值列仍分别对齐
        frame.render_widget(
            Paragraph::new(param_rows).alignment(Alignment::Center),
            chunks[1],
        );

        // 分隔线
        let sep = Line::from(Span::styled(
            " ".repeat(inner.width.saturating_sub(1) as usize),
            Style::new().fg(theme.border),
        ));
        frame.render_widget(Paragraph::new(sep), chunks[2]);

        // 按钮行（居中）
        frame.render_widget(
            Paragraph::new(self.button_row()).alignment(Alignment::Center),
            chunks[3],
        );

        // 操作提示（居中）
        let hint = Line::from(Span::styled(
            "↑↓ 选择  ←→ 调整  Enter 连接  Esc 取消",
            Style::new().fg(Color::DarkGray),
        ));
        frame.render_widget(Paragraph::new(hint).alignment(Alignment::Center), chunks[4]);

        // 当前选中串口的设备信息（按钮/提示下方空白处，左对齐）
        self.render_device_info(frame, chunks[5]);
    }

    /// 渲染当前选中串口的设备信息（芯片/厂商/VID-PID），无信息时显示占位
    fn render_device_info(&self, frame: &mut Frame, area: Rect) {
        let theme = &self.theme;
        let dev = self.port_candidates.get(self.port_idx);
        let info_lines: Vec<Line> = match dev {
            Some(d) => {
                let mut lines: Vec<Line> = Vec::new();
                // 设备类型
                lines.push(Line::from(vec![
                    Span::styled("类型: ", Style::new().fg(theme.sys)),
                    Span::styled(d.kind.to_string(), Style::new().fg(theme.fg)),
                ]));
                // 芯片型号
                if let Some(c) = &d.chip {
                    lines.push(Line::from(vec![
                        Span::styled("芯片: ", Style::new().fg(theme.sys)),
                        Span::styled(c.clone(), Style::new().fg(theme.fg)),
                    ]));
                }
                if let Some(m) = &d.manufacturer {
                    lines.push(Line::from(vec![
                        Span::styled("厂商: ", Style::new().fg(theme.sys)),
                        Span::styled(m.clone(), Style::new().fg(theme.fg)),
                    ]));
                }
                if let Some(p) = &d.product {
                    lines.push(Line::from(vec![
                        Span::styled("设备: ", Style::new().fg(theme.sys)),
                        Span::styled(p.clone(), Style::new().fg(theme.fg)),
                    ]));
                }
                if let (Some(v), Some(p)) = (&d.vid, &d.pid) {
                    lines.push(Line::from(vec![
                        Span::styled("VID/PID: ", Style::new().fg(theme.sys)),
                        Span::styled(format!("{:04} : {:04}", v, p), Style::new().fg(theme.fg)),
                    ]));
                }
                if let Some(s) = &d.serial {
                    lines.push(Line::from(vec![
                        Span::styled("序列号: ", Style::new().fg(theme.sys)),
                        Span::styled(s.clone(), Style::new().fg(theme.fg)),
                    ]));
                }
                if lines.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "无设备信息",
                        Style::new().fg(Color::DarkGray),
                    )));
                }
                lines
            }
            None => vec![Line::from(Span::styled(
                "无可用端口",
                Style::new().fg(Color::DarkGray),
            ))],
        };
        frame.render_widget(Paragraph::new(info_lines), area);
    }

    /// 参数行：标签固定宽 + 值，选中行整行反色高亮 + ▸ 指示。
    /// `width` 为内容区宽度（含箭头），行尾用空格补齐使各行等宽，
    /// 从而在整体居中时标签列与值列分别对齐。
    fn settings_row(&self, idx: usize, label: &str, value: &str, width: usize) -> Line<'static> {
        let theme = &self.theme;
        let selected = idx == self.settings_sel;
        // 标签固定 6 宽左对齐，值列从统一位置开始
        let lbl = format!("{:<6}", label);
        // 计算文本总显示宽（箭头 + 标签 + 间隔 + 值），用显示宽度（中文双宽）补齐等宽
        let arrow = if selected { "▸ " } else { "  " };
        let gap = "  ";
        let body = format!("{}{}{}", lbl, gap, value);
        // 箭头 2 宽 + 标签显示宽 + 间隔 2 + 值显示宽 = 本行显示宽
        let text_w = 2 + UnicodeWidthStr::width(lbl.as_str()) + 2 + UnicodeWidthStr::width(value);
        let pad = width.saturating_sub(text_w);
        if selected {
            let hl = Style::new()
                .fg(Color::Black)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD);
            Line::from(vec![
                Span::styled(arrow, Style::new().fg(theme.accent)),
                Span::styled(format!("{}{}", body, " ".repeat(pad)), hl),
            ])
        } else {
            Line::from(vec![
                Span::styled(arrow, Style::new()),
                Span::styled(
                    format!("{}{}", body, " ".repeat(pad)),
                    Style::new().fg(theme.fg),
                ),
            ])
        }
    }

    /// 按钮行：连接/取消两个按钮，选中项高亮
    fn button_row(&self) -> Line<'static> {
        let theme = &self.theme;
        let btn = |sel: bool| {
            if sel {
                Style::new()
                    .fg(Color::Black)
                    .bg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(theme.fg)
            }
        };
        Line::from(vec![
            Span::styled(" [ 连接 ] ", btn(self.settings_sel == 8)),
            Span::styled("   ", Style::new()),
            Span::styled("[ 取消 ]", btn(self.settings_sel == 9)),
        ])
    }

    // ---------- 日志导出与回放 ----------

    /// 导出当前输出区日志到文件（默认存 exe 同目录 serial_export_<时间>.log）
    fn export_logs(&mut self, path_arg: &str) -> Option<String> {
        if self.lines.is_empty() {
            self.push_line(Dir::Err, "当前没有可导出的日志");
            return None;
        }
        let path = if path_arg.is_empty() {
            // 未指定文件名：默认存到 exe 同目录
            resolve_output_path(&format!(
                "serial_export_{}.log",
                chrono::Local::now().format("%Y%m%d_%H%M%S")
            ))
        } else {
            // 指定文件名：相对路径也解析到 exe 同目录；
            // 若为绝对路径或含目录分隔符，则按用户给定路径处理。
            let p = std::path::Path::new(path_arg);
            let is_absolute = p.is_absolute();
            let has_dir = p.parent().is_some_and(|d| !d.as_os_str().is_empty());
            if is_absolute || has_dir {
                std::path::PathBuf::from(path_arg)
            } else {
                resolve_output_path(path_arg)
            }
        };
        let mut f = match File::create(&path) {
            Ok(f) => f,
            Err(e) => {
                self.push_line(Dir::Err, format!("导出失败: {}", e));
                return None;
            }
        };
        for l in &self.lines {
            let _ = writeln!(f, "{}", self.format_line(l));
        }
        Some(path.display().to_string())
    }

    /// 回放日志文件：解析 [时间戳] [RX|TX] 行，按原始时间间隔回放。
    /// 支持倍率：speed>1 加速、speed<1 减速、speed=1 原速。
    /// 回放运行期间在独立线程执行，不阻塞 UI，支持 :replay stop 停止。
    fn start_replay(&mut self, path_arg: &str, speed: f64) {
        if path_arg.is_empty() {
            self.push_line(Dir::Err, "用法: :replay <日志文件> [倍率]");
            return;
        }
        if self.replaying {
            self.push_line(Dir::Err, "已有回放在运行，输入 :replay stop 停止");
            return;
        }
        let speed = if speed > 0.0 { speed } else { 1.0 };
        // 解析路径：与应用目录一致（exe 同目录，纯文件名时）
        let p = std::path::Path::new(path_arg);
        let is_absolute = p.is_absolute();
        let has_dir = p.parent().is_some_and(|d| !d.as_os_str().is_empty());
        let path = if is_absolute || has_dir {
            std::path::PathBuf::from(path_arg)
        } else {
            // 纯拼接 exe 目录，不做可写性探测（避免清空/删除已有回放文件）
            exe_dir_join(path_arg)
        };
        // 启动前校验文件是否存在（不在线程内，界面立即反馈）
        if !path.exists() {
            self.push_line(Dir::Err, format!("回放文件不存在: {}", path.display()));
            return;
        }
        let tx_sender = self.tx_sender.clone();
        let rx_tx = self.rx_tx.clone();
        let speed_label = if speed != 1.0 {
            format!(" @ {}x", speed)
        } else {
            String::new()
        };
        self.push_line(
            Dir::Sys,
            format!("开始回放 {}{}（请勿手动发送）", path.display(), speed_label),
        );

        let stop = Arc::new(AtomicBool::new(false));
        let run = stop.clone();
        self.replay_stop = Some(stop);
        self.replaying = true;

        let handle = thread::spawn(move || {
            let data = match std::fs::read_to_string(&path) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("回放读取失败: {}", e);
                    return;
                }
            };
            let mut prev_ts: Option<f64> = None;
            for line in data.lines() {
                if run.load(Ordering::Relaxed) {
                    break;
                }
                let ts = extract_timestamp_seconds(line);
                // 解析 [HH:MM:SS.mmm] [TX ] / [RX ] / [SYS] / [ERR]
                let tag = if line.contains("[TX ]") {
                    "TX"
                } else if line.contains("[RX ]") {
                    "RX"
                } else if line.contains("[SYS]") {
                    "SYS"
                } else if line.contains("[ERR]") {
                    "ERR"
                } else {
                    ""
                };
                let body = extract_body(line).unwrap_or_default();
                // 按时间间隔等待（模拟原始时序，并按倍率加速/减速）
                if let (Some(t), Some(p)) = (ts, prev_ts)
                    && t > p
                {
                    std::thread::sleep(std::time::Duration::from_millis(
                        ((t - p) * 1000.0 / speed) as u64,
                    ));
                }
                if ts.is_some() {
                    prev_ts = ts;
                }
                // 每条记录都在输出区回显（RX/SYS 仅显示；TX 显示并真正发送到串口）
                match tag {
                    "TX" => {
                        // 回显 TX 行到输出区
                        let _ = rx_tx.send(SerialEvent::Replay(Dir::Tx, body.clone()));
                        // 转义还原后真正发送到串口
                        let bytes = if let Some(hex_body) = body.strip_prefix("(HEX) ") {
                            parse_hex(hex_body).unwrap_or_default()
                        } else {
                            unescape(&body).into_bytes()
                        };
                        if !bytes.is_empty() {
                            let _ = tx_sender.send(bytes);
                        }
                    }
                    "RX" => {
                        let _ = rx_tx.send(SerialEvent::Replay(Dir::Rx, body));
                    }
                    "SYS" => {
                        let _ =
                            rx_tx.send(SerialEvent::Replay(Dir::Sys, format!("(回放) {}", body)));
                    }
                    "ERR" => {
                        let _ = rx_tx.send(SerialEvent::Replay(Dir::Err, body));
                    }
                    _ => {}
                }
            }
            // 自然结束（非 stop 触发）时发送完成事件，保证位于所有回显之后（FIFO 顺序）
            if !run.load(Ordering::Relaxed) {
                let _ = rx_tx.send(SerialEvent::Replay(Dir::Sys, "回放完成".into()));
            }
        });
        self.replay_handle = Some(handle);
    }

    /// 停止回放：置停止标志并等待线程退出，复位回放状态
    fn stop_replay(&mut self) {
        if !self.replaying {
            self.push_line(Dir::Sys, "当前没有回放在运行");
            return;
        }
        if let Some(stop) = &self.replay_stop {
            stop.store(true, Ordering::Relaxed);
        }
        if let Some(h) = self.replay_handle.take() {
            for _ in 0..50 {
                if h.is_finished() {
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
        self.replay_stop = None;
        self.replaying = false;
        self.push_line(Dir::Sys, "回放已停止");
    }

    // ---------- 辅助 ----------

    fn save_cfg(&mut self) -> anyhow::Result<()> {
        self.cfg.cfg.port = self.port_name.clone();
        self.cfg.cfg.baud = self.baud;
        self.cfg.cfg.encoding = self.enc.as_str_name().to_string();
        self.cfg.cfg.mode = if self.mode == SendMode::Direct {
            "direct".into()
        } else {
            "line".into()
        };
        // 与 new() 的合并逻辑保持一致：把其余可持久化参数一并保存
        self.cfg.cfg.databits = self.databits;
        self.cfg.cfg.stopbits = self.stopbits.clone();
        self.cfg.cfg.parity = self.parity.clone();
        self.cfg.cfg.flow = self.flow.clone();
        self.cfg.cfg.theme = if self.theme == Theme::dracula() {
            "dracula".into()
        } else {
            "github-dark".into()
        };
        self.cfg.cfg.history = self.history.clone();
        self.cfg.save()
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1])[1]
}
