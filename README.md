# Serial Terminal

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Build Windows](https://img.shields.io/github/actions/workflow/status/kairui1108/SerialTerminal/build-windows.yml?label=build%20windows)](https://github.com/kairui1108/SerialTerminal/actions)

一个用 **Rust** 实现的串口终端，专为 **Windows Terminal** 深度集成而设计。


## ✨ 功能特性

- **串口连接配置**：支持端口、波特率、数据位、停止位、校验位、流控的完整配置，提供图形化设置面板与命令行双入口
- **中文乱码**：内置 `encoding_rs` 流式解码，支持 UTF-8 / GBK / 自动检测 / 十六进制（Hex）视图，跨数据块拆分的中文字符也不会乱码
- **实时收发**：独立的读/写线程，读写句柄分离，长写超时避免设备繁忙时写入失败，界面永不卡顿
- **Windows Terminal 原生体验**：作为 Windows Terminal Profile 启动，与终端标签页、快捷键、鼠标操作无缝融合
- **现代 TUI 界面**：三区布局（状态栏 / 输出区 / 输入区），内置 8 套配色主题（GitHub Dark、Dracula、Monokai、Solarized Dark、Nord、One Dark、Gruvbox、Tokyo Night），长行自动折行，鼠标滚轮浏览历史
- **设备信息识别**：自动识别常见 USB 串口芯片（CH340、CP210x、FTDI、PL2303、ESP32 全系、STM32、RP2040 等 100+ 款），显示芯片型号、厂商与 VID/PID
- **命令系统**：纯命令驱动，支持动态补全（命令名、参数、串口名、日志文件四级补全）
- **日志能力**：`:log` 实时记录、`:export` 导出日志、`:replay` 按原始时序回放（支持倍速）
- **搜索定位**：`:search` / `:find` 在历史日志中搜索关键词并高亮定位
- **最近设备记忆**：自动记录最近使用的设备参数，`:recent` 一键重连
- **断线自动重连**：设备拔出/重插自动恢复连接
- **单文件便携**：单个 `serial-term.exe`，免安装、免注册表

---

## 🚀 安装

### 系统要求

- **Windows 10 / 11**
- **Windows Terminal**（推荐，从 Microsoft Store 安装）

### 安装步骤

1. 从 [Releases](../../releases) 下载 `serial-term.exe`（Windows x64）。
2. 将 `serial-term.exe` 放到任意目录（建议放入固定目录，如 `C:\Tools\serial-term\`）。
3. 可直接双击运行，或通过命令行 / Windows Terminal 启动。

> 便携模式：程序自动在 exe 同目录读写 `config.toml`（若该目录可写），无需注册表。

### 从源码构建

```powershell
git clone https://github.com/kairui1108/SerialTerminal.git
cd SerialTerminal
cargo build --release
# 二进制位于 target/release/serial-term.exe
```

---

## 🎯 快速开始

```powershell
# 列出可用串口
serial-term.exe --list-ports

# 直接连接
serial-term.exe --port COM3 --baud 115200 --encoding auto

# 显示帮助
serial-term.exe --help
```

启动后默认弹出**串口设置面板**，可手动选择端口与参数后连接。

### 集成 Windows Terminal（推荐）

```powershell
serial-term.exe --install-profile
```

程序会打印 Windows Terminal 的 profile 配置片段，将其粘贴到 Windows Terminal 设置（`Ctrl+,`）→ 打开 JSON 文件 → 添加到 `profiles.list` 中：

```json
{
  "name": "Serial COM3",
  "commandline": "serial-term.exe --port COM3 --baud 115200 --encoding auto",
  "suppressApplicationTitle": true
}
```

保存后，Windows Terminal 的下拉菜单即会出现"Serial COM3"选项，点击即可连接串口。

**配置多个串口**：可以为不同端口/参数创建多个 Profile，实现一键切换。

---

## ⌨️ 使用方法

### 命令行参数

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `--port <p>` | 串口端口（如 `COM3`） | 自动探测 |
| `--baud <n>` | 波特率 | `115200` |
| `--databits <5-8>` | 数据位 | `8` |
| `--stopbits <1\|2>` | 停止位 | `1` |
| `--parity <none\|odd\|even>` | 校验位 | `none` |
| `--flow <none\|software\|hardware>` | 流控 | `none` |
| `--encoding <utf8\|gbk\|auto\|raw>` | 编码 | `utf8` |
| `--mode <line\|direct>` | 发送模式 | `line` |
| `--ts <off\|short\|iso>` | 时间戳格式 | `short` |
| `--no-tags` | 隐藏 RX/TX 标签列 | 关闭 |
| `--flush-ms <n>` | 半行强制刷行超时（毫秒） | `200` |
| `--tx-on-enter <CR\|LF\|CRLF>` | 回车发送的行尾 | `CRLF` |
| `--keep-input` | 发送后保留输入内容 | 关闭 |
| `--theme <主题>` | 配色主题（8 套） | `github-dark` |
| `--auto-connect` | 启动时自动连接 | 关闭 |
| `--list-ports` | 列出可用串口并退出 | - |
| `--install-profile` | 打印 Windows Terminal 配置片段 | - |
| `--no-config` | 不读写配置文件 | 关闭 |
| `--config <path>` | 指定配置文件路径 | exe 同目录 |
| `--verbose` | 输出调试日志 | 关闭 |

### 内置命令

输入框以 `:` 开头输入命令，按 `Enter` 执行（支持 Tab / ↑↓ 补全）。

**连接管理**

| 命令 | 说明 |
|------|------|
| `:port <p>` / `:connect <p>` | 连接串口 |
| `:close` / `:disconnect` | 关闭串口 |
| `:list` | 列出可用串口 |
| `:settings` / `:config` | 打开设置面板 |
| `:status` | 显示连接状态 |
| `:baud <n>` / `:databits` / `:stopbits` / `:parity` / `:flow` | 调整串口参数并重连 |
| `:recent [序号]` | 最近设备记录 / 一键重连 |
| `:reconnect [端口\|off]` / `:auto` | 断线自动重连 |

**显示控制**

| 命令 | 说明 |
|------|------|
| `:tail` / `:live` / `:follow` | 回到最新输出 |
| `:top` | 回到输出顶部 |
| `:up` / `:down` | 上滑 / 下滑历史 |
| `:scroll up\|down\|<行数>` | 滚动输出历史 |
| `:clear` | 清屏 |
| `:search <关键词> [序号]` / `:find` | 搜索并高亮定位历史日志 |
| `:hex` / `:text` | 十六进制 / 文本视图 |
| `:ts on\|off\|iso` | 时间戳开关 |
| `:encoding <utf8\|gbk\|auto\|raw>` | 编码切换 |
| `:mode line\|direct` | 发送模式切换 |
| `:theme [主题]` | 切换配色主题 |

**发送 / 日志**

| 命令 | 说明 |
|------|------|
| `:send "<文本>"` | 发送文本（含转义） |
| `:hex 41 42 43` | 十六进制发送 |
| `:log on\|off` | 实时日志记录 |
| `:export [文件]` | 导出当前日志 |
| `:replay <文件 [倍率]\|stop>` | 日志回放 / 停止 |
| `:help` | 帮助 |
| `:quit` / `:exit` | 退出程序 |

### 快捷键

| 按键 | 功能 |
|------|------|
| `Enter` | 发送（行缓冲模式） |
| `Alt+Enter` | 输入换行 |
| `↑` / `↓` | 发送历史 / 补全候选选择 |
| `Tab` | 命令 / 参数补全 |
| `Esc` | 清空输入 / 关闭弹窗 |
| `PgUp` / `PgDn` | 输出区翻页 |
| 鼠标滚轮 | 浏览输出历史 |
| `Shift+拖选` | 复制文本 |
| `Ctrl+Shift+C` / `Ctrl+Shift+V` | 复制 / 粘贴（Windows Terminal） |

### 配色主题

内置 8 套配色，通过 `:theme <名称>` 或 `--theme <名称>` 切换：

| 主题 | 说明 |
|------|------|
| `github-dark` | GitHub 暗色（默认） |
| `dracula` | 德古拉紫 |
| `monokai` | Monokai 高对比 |
| `solarized-dark` | Solarized 暗色低饱和 |
| `nord` | 极简冷色 |
| `one-dark` | Atom One Dark |
| `gruvbox` | 复古暖色 |
| `tokyo-night` | 东京夜 |

主题选择会持久化到配置文件，下次启动自动沿用。

### 配置说明

程序自动在 exe 同目录（可写时）读写 `config.toml`，记录最近端口、参数、主题、发送历史与设备记录。使用 `--no-config` 可完全禁用，或用 `--config <path>` 指定配置文件路径。

---

### 开发环境

- Windows 10/11 + Rust 1.85+（edition 2024）
- 推荐使用 Windows Terminal 进行开发测试


## 📄 许可证

本项目基于 [MIT](LICENSE) 许可证开源。

## 🙏 致谢

- [Zhou-zhi-peng/SerialPortForWindowsTerminal](https://github.com/Zhou-zhi-peng/SerialPortForWindowsTerminal) — Windows Terminal 串口插件
- [ratatui](https://github.com/ratatui/ratatui) — 终端用户界面框架
- [serialport-rs](https://github.com/serialport/serialport-rs) — 串口库
- [Windows Terminal](https://github.com/microsoft/terminal) — 集成终端
