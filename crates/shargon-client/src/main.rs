mod arguments;
mod cli_command;
mod daemon;

use arguments::prelude::*;
use cli_command::*;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli_args = arguments::Arguments::parse();

    let cmd: Box<dyn cli_command::CliCommand> = match cli_args.command {
        arguments::Command::Cancel { task_id } => Box::new(CliCancelCommand::new(task_id)),
        arguments::Command::Machines => Box::new(CliMachinesCommand::new()),
        arguments::Command::Ping => Box::new(CliPingCommand::new()),
        arguments::Command::Run { argv } => Box::new(CliRunCommand::new(argv)),
        arguments::Command::TaskStatus { task_id } => {
            Box::new(CliTaskStatusCommand::new(task_id))
        }
        arguments::Command::Tasks => Box::new(CliTasksCommand::new()),
        arguments::Command::Version => Box::new(CliVersionCommand::new()),
    };

    cmd.execute()?;

    Ok(())
}
