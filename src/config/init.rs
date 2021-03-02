use crate::error::Result;

impl super::InitConfig {
    pub(super) fn execute(&self) -> Result<()> {
        log::info!("Init ...");
        let _ = self.config.accounts()?;
        self.storage.put_metadata(&self.config)?;
        Ok(())
    }
}
