mod git;
mod stale;

use clap::Parser;

#[derive(Parser)]
#[command(name = "ship")]
#[command(about = "nudge the commit・merge・push・plugin-update shipping ritual")]
struct Cli {
    // placeholder; real subcommands added in ship-cli-and-safety task
}

fn main() {
    let _ = Cli::parse();
}
