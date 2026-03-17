#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    Command { command: Vec<String> },
    Port { port: u16 },
}

#[derive(Debug, PartialEq, Eq)]
pub struct Config {
    pub origin: Option<String>,
    pub user: Option<String>,
    pub mode: Mode,
}
