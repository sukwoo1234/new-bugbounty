use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "tool", version, about = "Bug bounty fuzzing platform CLI")]
struct Cli {
    /// Data directory (default: ./data)
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,

    /// Seeds directory (default: ./seeds)
    #[arg(long, default_value = "./seeds")]
    seeds_dir: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run(RunArgs),
    Triage(TriageArgs),
    Report(ReportArgs),
    List(ListArgs),
    Show(ShowArgs),
    Export(ExportArgs),
}

#[derive(Args)]
struct RunArgs {}

#[derive(Args)]
struct TriageArgs {
    /// Input file to reproduce
    #[arg(long)]
    input: PathBuf,
}

#[derive(Args)]
struct ReportArgs {}

#[derive(Args)]
struct ListArgs {}

#[derive(Args)]
struct ShowArgs {
    /// Result ID to show
    id: String,
}

#[derive(Args)]
struct ExportArgs {
    /// Result ID to export
    id: String,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run(_args) => {
            print_stub("run", &cli.data_dir, &cli.seeds_dir);
        }
        Commands::Triage(args) => {
            print_stub_with_input("triage", &cli.data_dir, &cli.seeds_dir, &args.input);
        }
        Commands::Report(_args) => {
            print_stub("report", &cli.data_dir, &cli.seeds_dir);
        }
        Commands::List(_args) => {
            print_stub("list", &cli.data_dir, &cli.seeds_dir);
        }
        Commands::Show(args) => {
            print_stub_with_id("show", &cli.data_dir, &cli.seeds_dir, &args.id);
        }
        Commands::Export(args) => {
            print_stub_with_id("export", &cli.data_dir, &cli.seeds_dir, &args.id);
        }
    }
}

fn print_stub(command: &str, data_dir: &PathBuf, seeds_dir: &PathBuf) {
    println!("[{}] not implemented yet", command);
    println!("data_dir: {}", data_dir.display());
    println!("seeds_dir: {}", seeds_dir.display());
}

fn print_stub_with_input(command: &str, data_dir: &PathBuf, seeds_dir: &PathBuf, input: &PathBuf) {
    println!("[{}] not implemented yet", command);
    println!("input: {}", input.display());
    println!("data_dir: {}", data_dir.display());
    println!("seeds_dir: {}", seeds_dir.display());
}

fn print_stub_with_id(command: &str, data_dir: &PathBuf, seeds_dir: &PathBuf, id: &str) {
    println!("[{}] not implemented yet", command);
    println!("id: {}", id);
    println!("data_dir: {}", data_dir.display());
    println!("seeds_dir: {}", seeds_dir.display());
}
