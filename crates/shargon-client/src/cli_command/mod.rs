mod ping;
mod run;
mod version;

pub use ping::CliPingCommand;
pub use run::CliRunCommand;
pub use version::CliVersionCommand;

pub trait CliCommand {
    fn execute(&self) -> anyhow::Result<()>;
}
