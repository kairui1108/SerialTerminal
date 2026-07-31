use clap::Parser;
use std::path::PathBuf;

/// 跨平台串口终端，可无缝集成 Windows Terminal
#[derive(Debug, Parser)]
#[command(name = "serial-term", version, about)]
pub struct Cli {
    /// 串口端口，如 COM4 或 /dev/ttyUSB0（缺省时读配置或自动探测）
    #[arg(long)]
    pub port: Option<String>,

    /// 波特率
    #[arg(long, default_value_t = 115200)]
    pub baud: u32,

    /// 数据位 (5-8)
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u8).range(5..=8))]
    pub databits: u8,

    /// 停止位: 1 / 2
    #[arg(long, default_value = "1", value_parser = ["1", "2"])]
    pub stopbits: String,

    /// 校验位: none / odd / even
    #[arg(long, default_value = "none", value_parser = ["none", "odd", "even"])]
    pub parity: String,

    /// 流控: none / software / hardware
    #[arg(long, default_value = "none", value_parser = ["none", "software", "hardware"])]
    pub flow: String,

    /// 编码: utf8 / gbk / auto / raw
    #[arg(long, default_value = "utf8", value_parser = ["utf8", "gbk", "auto", "raw"])]
    pub encoding: String,

    /// 发送模式: line(行缓冲) / direct(逐字符)
    #[arg(long, default_value = "line", value_parser = ["line", "direct"])]
    pub mode: String,

    /// 时间戳: off / short / iso
    #[arg(long, default_value = "short", value_parser = ["off", "short", "iso"])]
    pub ts: String,

    /// 隐藏 RX/TX 标签列
    #[arg(long)]
    pub no_tags: bool,

    /// 半行强制刷行超时(毫秒)
    #[arg(long, default_value_t = 200)]
    pub flush_ms: u64,

    /// 行缓冲模式回车发送的行尾: CR / LF / CRLF
    #[arg(long, default_value = "CRLF", value_parser = ["CR", "LF", "CRLF"])]
    pub tx_on_enter: String,

    /// 发送后保留输入内容
    #[arg(long)]
    pub keep_input: bool,

    /// 主题: github-dark / dracula
    #[arg(long, default_value = "github-dark")]
    pub theme: String,

    /// 启动时自动连接（默认进入端口选择，需手动确认）
    #[arg(long)]
    pub auto_connect: bool,

    /// 不使用配置文件（纯内存运行）
    #[arg(long)]
    pub no_config: bool,

    /// 指定配置文件路径
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// 列出可用串口并退出
    #[arg(long)]
    pub list_ports: bool,

    /// 打印 Windows Terminal profile 配置片段
    #[arg(long)]
    pub install_profile: bool,

    /// 输出调试日志
    #[arg(long)]
    pub verbose: bool,
}
