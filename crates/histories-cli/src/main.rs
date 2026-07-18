use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "histories",
    about = "Search and export local AI conversation histories"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show the configured data sources.
    Sources,
}

fn main() {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Sources) {
        Command::Sources => println!("Codex, Pi, ChatGPT export"),
    }
}
