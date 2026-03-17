#[derive(Debug, PartialEq, Eq)]
pub struct Config {
    pub origin: Option<String>,
    pub user: Option<String>,
    pub command: Vec<String>,
}
