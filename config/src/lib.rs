use serde::Deserialize;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

#[derive(Deserialize)]
pub struct Config {
    pub webhooks: WebHookConfig,
    pub dispatch: DispatchConfig,
    pub clients: HashMap<String, ClientConfig>,
}

#[derive(Deserialize)]
pub struct WebHookConfig {
    pub pipe: PathBuf,
    pub listen_addr: std::net::Ipv4Addr,
    pub listen_port: u16,
}

#[derive(Deserialize)]
pub struct DispatchConfig {
    pub pipe: PathBuf,
    pub scripts_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClientConfig {
    pub secret: String,
    pub project: String,
    pub permissions: HashSet<Action>,
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Deploy,
}

#[derive(Debug, Clone)]
pub struct Command {
    pub action: Action,
    pub project: String,
}

pub struct CommandParseError;

impl std::str::FromStr for Command {
    type Err = CommandParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (action, project) = s.split_once(" ").ok_or(CommandParseError)?;

        let action = match action {
            "deploy" => Action::Deploy,
            _ => return Err(CommandParseError),
        };

        Ok(Command {
            action,
            project: project.to_string(),
        })
    }
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let action = match self {
            Action::Deploy => "deploy",
        };
        write!(f, "{}", action)
    }
}

impl std::fmt::Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.action, self.project)
    }
}
