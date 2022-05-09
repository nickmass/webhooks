use clap::Parser;
use config::Config;

use std::collections::HashSet;
use std::io::BufRead;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[clap(long, default_value = "config.toml")]
    config: PathBuf,
}

fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    tracing::info!("loading config from: {}", args.config.display());

    let config_file = std::fs::read_to_string(args.config).unwrap();
    let config: Config = toml::from_str(&config_file).unwrap();
    let config: &'static Config = Box::leak(Box::new(config));

    tracing::info!("opening pipe: {}", config.dispatch.pipe.display());

    let projects: HashSet<String> = config
        .clients
        .values()
        .map(|client| client.project.clone())
        .collect();

    loop {
        let pipe = std::fs::OpenOptions::new()
            .read(true)
            .open(&config.dispatch.pipe)
            .unwrap();
        let pipe = std::io::BufReader::new(pipe);

        for line in pipe.lines() {
            let line = match line {
                Ok(line) => line,
                Err(err) => {
                    tracing::error!("error reading from pipe: {err:?}");
                    continue;
                }
            };
            tracing::info!("got line: {line}");

            let command: config::Command = match line.parse() {
                Ok(command) => command,
                Err(_err) => {
                    tracing::error!("unable to parse command");
                    continue;
                }
            };
            tracing::info!("got command: {command}");

            if projects.contains(&command.project) {
                let mut path = PathBuf::from(&config.dispatch.scripts_dir);
                path.push(command.project);
                path.push(command.action.to_string());

                tracing::info!("executing command: {}", path.display());
                let mut command = std::process::Command::new(path);
                match command.status() {
                    Ok(status) => {
                        tracing::info!("command completed with status: {}", status);
                    }
                    Err(err) => tracing::error!("unabled to execute command: {err:?}"),
                }
            } else {
                tracing::error!(
                    "recieved command for unconfigured project: {}",
                    command.project
                );
            }
        }
    }
}
