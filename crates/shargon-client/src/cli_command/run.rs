use super::CliCommand;

pub struct CliRunCommand {}

impl CliRunCommand {
    pub fn new() -> Self {
        Self {}
    }
}

impl CliCommand for CliRunCommand {
    fn execute(&self) -> anyhow::Result<()> {
        println!("run");
        Ok(())
    }
}
