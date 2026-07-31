use crate::cli::Cli;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 最近使用的设备连接记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentDevice {
    pub port: String,
    pub baud: u32,
    pub databits: u8,
    pub stopbits: String,
    pub parity: String,
    pub flow: String,
    pub encoding: String,
    pub last_used: String, // ISO 时间戳
    pub conn_count: u64,
}

/// 持久化配置（TOML 文件，便携优先：exe 同目录 -> 用户配置目录）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)] // 配置缺失字段时逐字段回退默认，而非整个配置静默重置
pub struct PersistConfig {
    pub port: Option<String>,
    pub baud: u32,
    pub databits: u8,
    pub stopbits: String,
    pub parity: String,
    pub flow: String,
    pub encoding: String,
    pub mode: String,
    pub theme: String,
    pub history: Vec<String>,
    /// 最近使用设备列表（按 last_used 倒序）
    pub recent: Vec<RecentDevice>,
}

impl Default for PersistConfig {
    fn default() -> Self {
        Self {
            port: None,
            baud: 115200,
            databits: 8,
            stopbits: "1".into(),
            parity: "none".into(),
            flow: "none".into(),
            encoding: "utf8".into(),
            mode: "line".into(),
            theme: "github-dark".into(),
            history: Vec::new(),
            recent: Vec::new(),
        }
    }
}

pub struct ConfigManager {
    pub path: Option<PathBuf>,
    pub cfg: PersistConfig,
}

impl ConfigManager {
    pub fn load(cli: &Cli) -> anyhow::Result<Self> {
        let path = resolve_path(cli)?;
        let cfg = match &path {
            Some(p) if p.exists() => {
                let s = fs::read_to_string(p)
                    .with_context(|| format!("读取配置文件失败: {}", p.display()))?;
                toml::from_str(&s).unwrap_or_else(|e| {
                    eprintln!("配置文件解析失败({}), 使用默认配置: {}", e, p.display());
                    PersistConfig::default()
                })
            }
            _ => PersistConfig::default(),
        };
        Ok(Self { path, cfg })
    }

    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(p) = &self.path {
            if let Some(dir) = p.parent() {
                fs::create_dir_all(dir)
                    .with_context(|| format!("创建配置目录失败: {}", dir.display()))?;
            }
            let s = toml::to_string_pretty(&self.cfg)?;
            fs::write(p, s).with_context(|| format!("写入配置文件失败: {}", p.display()))?;
        }
        Ok(())
    }
}

/// 解析配置文件路径：显式指定 -> exe 同目录(便携) -> 用户配置目录 -> None
fn resolve_path(cli: &Cli) -> anyhow::Result<Option<PathBuf>> {
    if cli.no_config {
        return Ok(None);
    }
    if let Some(p) = &cli.config {
        return Ok(Some(p.clone()));
    }
    // 便携优先：exe 同目录可写则使用
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("config.toml");
        if candidate.exists() || dir_writable(dir) {
            return Ok(Some(candidate));
        }
    }
    // 回退到用户配置目录
    if let Some(dirs) = directories::ProjectDirs::from("com", "serial-term", "serial-term") {
        return Ok(Some(dirs.config_dir().join("config.toml")));
    }
    Ok(None)
}

fn dir_writable(dir: &Path) -> bool {
    // create_new 原子创建（文件已存在则失败），配合随机后缀避免并发实例冲突
    let tmp = dir.join(format!(
        ".wtest_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
    {
        Ok(f) => {
            drop(f);
            let _ = fs::remove_file(&tmp);
            true
        }
        Err(_) => false,
    }
}
