use clap::Parser;

#[tokio::main]
async fn main() {
    let cli = tplc::cli::Cli::parse();
    let exit_code = tplc::run(cli).await;
    std::process::exit(exit_code);
}
