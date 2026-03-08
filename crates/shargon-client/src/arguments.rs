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
    Run {
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        argv: Vec<String>,
    },
    Tasks,
    TaskStatus {
        task_id: String,
    },
    Cancel {
        task_id: String,
    },
    Machines,
    Version,
}
