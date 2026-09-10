use clap::Parser;
use pk_cli_core::output;

fn main() {
    let cli = tplc::cli::Cli::parse();
    let code = match tplc::run(&cli) {
        Ok(code) => code,
        Err(e) => output::fail(&e, cli.common.json),
    };
    std::process::exit(code);
}
