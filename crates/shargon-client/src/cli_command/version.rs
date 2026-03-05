use super::CliCommand;

pub struct CliVersionCommand {}

impl CliVersionCommand {
    pub fn new() -> Self {
        Self {}
    }
}

impl CliCommand for CliVersionCommand {
    fn execute(&self) -> anyhow::Result<()> {
        println!("version");
        Ok(())
    }
}
