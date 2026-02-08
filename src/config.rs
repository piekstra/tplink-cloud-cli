#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Json,
    Table,
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub output_mode: OutputMode,
    pub verbose: bool,
}
