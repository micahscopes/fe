#[derive(Debug, serde::Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub(crate) enable_diagnostic_workers: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enable_diagnostic_workers: false,
        }
    }
}
