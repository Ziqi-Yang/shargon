mod run;
mod version;

pub use run::CliRunCommand;
pub use version::CliVersionCommand;

pub trait CliCommand {
    fn execute(&self) -> anyhow::Result<()>;
}
