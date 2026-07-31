mod app;
mod cli;
mod config;
mod serial;
mod term;

use clap::Parser;

fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();

    if cli.verbose {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    }

    if cli.list_ports {
        let ports = serial::port::list_ports();
        if ports.is_empty() {
            println!("未检测到可用串口");
        } else {
            for p in &ports {
                match p.summary() {
                    Some(info) => println!("{}  [{}]", p.name(), info),
                    None => println!("{}", p.name()),
                }
            }
        }
        return Ok(());
    }

    if cli.install_profile {
        print_profile(&cli);
        return Ok(());
    }

    term::console::setup_console()?;
    let cfg = config::ConfigManager::load(&cli)?;
    let engine = app::engine::Engine::new(cli, cfg)?;
    engine.run()
}

#[cfg(windows)]
fn print_profile(cli: &cli::Cli) {
    let mut args = format!("--baud {}", cli.baud);
    if let Some(p) = &cli.port {
        args.push_str(&format!(" --port {}", p));
    }
    args.push_str(&format!(
        " --parity {} --databits {} --stopbits {} --encoding {} --mode {} --ts {}",
        cli.parity, cli.databits, cli.stopbits, cli.encoding, cli.mode, cli.ts
    ));
    println!();
    println!("将以下 profile 添加到 Windows Terminal settings.json 的 profiles.list:");
    println!(
        r#"{{
    "name": "Serial Terminal",
    "commandline": "serial-term.exe {}",
    "suppressApplicationTitle": true
}}"#,
        args
    );
}

#[cfg(not(windows))]
fn print_profile(_: &cli::Cli) {
    println!(
        "--install-profile 仅用于 Windows + Windows Terminal；当前平台直接在任何终端中运行即可。"
    );
}
