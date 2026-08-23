use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use gyr_model::builtin_profiles;

#[derive(Debug, Parser)]
#[command(name = "gyr", version, about = "A Rust-first terminal coding agent")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List the deliberately bounded built-in model catalogue.
    Models {
        /// Emit the full catalogue as JSON.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Models { json: true } => {
            println!("{}", serde_json::to_string_pretty(&builtin_profiles())?);
        }
        Command::Models { json: false } => {
            for profile in builtin_profiles() {
                println!(
                    "{:<24} {:<12} {}",
                    profile.key,
                    format!("{:?}", profile.provider).to_lowercase(),
                    profile.provider_model
                );
            }
        }
    }

    Ok(())
}
