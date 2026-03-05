use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
struct Arguments {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    Version,
}

fn main() {
    let args = Arguments::parse();

    match args.command {
        Some(Command::Version) => {
            println!("{}", shargon_version::current_version_line!());
        }
        None => {
            println!("daemon");
        }
    }
}
