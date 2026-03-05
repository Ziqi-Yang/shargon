mod arguments;
mod cli_command;

use arguments::prelude::*;
use cli_command::*;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli_args = arguments::Arguments::parse();

    let cmd: Box<dyn cli_command::CliCommand> = match cli_args.command {
        arguments::Command::Ping => Box::new(CliPingCommand::new()),
        arguments::Command::Run => Box::new(CliRunCommand::new()),
        arguments::Command::Version => Box::new(CliVersionCommand::new()),
    };

    cmd.execute()?;

    Ok(())
}
