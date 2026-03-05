use clap::{Parser, Subcommand};

pub mod prelude {
    pub use clap::Parser;
}

#[derive(Parser, Debug)]
pub struct Arguments {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Ping,
    Run,
    Version,
}
