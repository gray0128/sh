use clap::{Arg, Command, ArgMatches};
use std::process;

mod setup_ssh;
mod singbox;
mod utils;

use setup_ssh::handle_setup_ssh;
use singbox::handle_singbox;
use utils::{CliError, OutputFormat};

fn build_cli() -> Command {
    Command::new("vps-cli")
        .about("管理 VPS SSH 和 sing-box 的工具，支持交互式和非交互模式")
        .version(env!("CARGO_PKG_VERSION"))
        .arg(
            Arg::new("json")
                .long("json")
                .help("输出 JSON 格式，便于代理解析")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("plain")
                .long("plain")
                .help("输出纯文本表格，不使用颜色和宽表")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("no-input")
                .long("no-input")
                .help("禁止交互式输入，如果需要的参数缺失则直接报错")
                .action(clap::ArgAction::SetTrue),
        )
        .subcommand(setup_ssh::cli())
        .subcommand(singbox::cli())
}

fn main() {
    let cli = build_cli();
    let matches = cli.get_matches();
    // Determine global output format
    let json = matches.get_flag("json");
    let plain = matches.get_flag("plain");
    let no_input = matches.get_flag("no-input");
    let output_format = if json {
        OutputFormat::Json
    } else if plain {
        OutputFormat::Plain
    } else {
        OutputFormat::Human
    };

    match matches.subcommand() {
        Some(("setup-ssh", sub)) => {
            if let Err(err) = handle_setup_ssh(sub, output_format, no_input) {
                err.output_and_exit(output_format);
            }
        }
        Some(("singbox", sub)) => {
            if let Err(err) = handle_singbox(sub, output_format, no_input) {
                err.output_and_exit(output_format);
            }
        }
        _ => {
            // Print help if no subcommand
            let _ = cli.print_help();
            println!();
        }
    }
}
