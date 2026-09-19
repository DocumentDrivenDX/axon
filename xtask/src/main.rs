use anyhow::Result;
use clap::{Parser, Subcommand};
use xtask::audit_dml_boundary::{self, AuditArgs};

#[derive(Debug, Parser)]
#[command(name = "xtask")]
#[command(about = "Axon workspace automation tasks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(name = "audit-dml-boundary")]
    #[command(about = "Audit governed SQL DML boundary manifest coverage")]
    AuditDmlBoundary(AuditArgs),
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::AuditDmlBoundary(args) => {
            let report = audit_dml_boundary::run(args)?;
            if report.is_success() {
                println!(
                    "audit-dml-boundary: checked {} governed SQL records",
                    report.records.len()
                );
            } else {
                eprintln!("{report}");
                std::process::exit(1);
            }
        }
    }

    Ok(())
}
