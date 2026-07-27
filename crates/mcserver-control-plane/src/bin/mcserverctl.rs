use std::{collections::BTreeMap, env, error::Error, path::PathBuf};

use mcserver_control_plane::client::UnixRpcClient;
use mcserver_protocol::client::method;
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

const DEFAULT_SOCKET_PATH: &str = "/run/mcserver/control-plane.sock";
const DEFAULT_CONTAINER_IMAGE: &str = "docker.io/itzg/minecraft-server:latest";
const DEFAULT_SERVER_TYPE: &str = "VANILLA";
const DEFAULT_MINECRAFT_VERSION: &str = "LATEST";
const DEFAULT_HOST_PORT: u16 = 25565;
const DEFAULT_STOP_TIMEOUT_SECONDS: u64 = 60;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if matches!(arguments.as_slice(), [argument] if argument == "--version" || argument == "-V") {
        println!("mcserverctl {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let invocation = Invocation::parse(arguments)?;
    let client = UnixRpcClient::new(invocation.socket_path);
    let result = execute(&client, invocation.command).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn execute(client: &UnixRpcClient, command: Command) -> Result<Value, CliError> {
    match command {
        Command::Ping => client
            .call_value(method::SYSTEM_PING, Value::Null)
            .await
            .map_err(Into::into),
        Command::ServerList => client
            .call_value(method::SERVER_LIST, Value::Null)
            .await
            .map_err(Into::into),
        Command::ServerGet { server_id } => {
            call_server_id(client, method::SERVER_GET, server_id).await
        }
        Command::ServerStatus { server_id } => {
            call_server_id(client, method::SERVER_STATUS, server_id).await
        }
        Command::ServerInstances { server_id } => {
            call_server_id(client, method::SERVER_INSTANCE_LIST, server_id).await
        }
        Command::ServerStart { server_id } => set_desired_state(client, server_id, "running").await,
        Command::ServerStop { server_id } => set_desired_state(client, server_id, "stopped").await,
        Command::ServerCreate(options) => {
            let options = *options;
            let compute = match options.compute {
                CreateCompute::Local => json!({ "provider": "local" }),
                CreateCompute::Akamai {
                    region,
                    instance_type,
                    image,
                    firewall_id,
                } => json!({
                    "provider": "akamai",
                    "region": region,
                    "instance_type": instance_type,
                    "image": image,
                    "firewall_id": firewall_id,
                }),
            };
            let params = json!({
                "name": options.name,
                "spec": {
                    "compute": compute,
                    "process": {
                        "container_image": options.container_image,
                        "server_type": options.server_type,
                        "version": options.version,
                        "host_port": options.host_port,
                        "stop_timeout_seconds": options.stop_timeout_seconds,
                        "accept_eula": true,
                        "environment": options.environment,
                    },
                    "data": { "repository": options.repository },
                },
            });
            client
                .call_value(method::SERVER_CREATE, params)
                .await
                .map_err(Into::into)
        }
    }
}

async fn call_server_id(
    client: &UnixRpcClient,
    method_name: &str,
    server_id: Uuid,
) -> Result<Value, CliError> {
    client
        .call_value(method_name, json!({ "server_id": server_id }))
        .await
        .map_err(Into::into)
}

async fn set_desired_state(
    client: &UnixRpcClient,
    server_id: Uuid,
    desired_state: &str,
) -> Result<Value, CliError> {
    let server = call_server_id(client, method::SERVER_GET, server_id).await?;
    let generation = server
        .get("generation")
        .and_then(Value::as_u64)
        .ok_or(CliError::InvalidServerResponse)?;
    client
        .call_value(
            method::SERVER_SET_DESIRED_STATE,
            json!({
                "server_id": server_id,
                "desired_state": desired_state,
                "expected_generation": generation,
            }),
        )
        .await
        .map_err(Into::into)
}

struct Invocation {
    socket_path: PathBuf,
    command: Command,
}

impl Invocation {
    fn parse(arguments: Vec<String>) -> Result<Self, CliError> {
        let mut arguments = arguments.into_iter();
        let mut socket_path = env::var_os("MCSERVER_CONTROL_PLANE_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_SOCKET_PATH));
        let mut positional = Vec::new();
        while let Some(argument) = arguments.next() {
            if argument == "--socket" {
                socket_path = PathBuf::from(
                    arguments
                        .next()
                        .ok_or(CliError::MissingFlagValue("--socket"))?,
                );
            } else if argument == "--help" || argument == "-h" {
                return Err(CliError::Usage);
            } else {
                positional.push(argument);
                positional.extend(arguments);
                break;
            }
        }
        let command = parse_command(positional)?;
        Ok(Self {
            socket_path,
            command,
        })
    }
}

enum Command {
    Ping,
    ServerList,
    ServerGet { server_id: Uuid },
    ServerStatus { server_id: Uuid },
    ServerInstances { server_id: Uuid },
    ServerStart { server_id: Uuid },
    ServerStop { server_id: Uuid },
    ServerCreate(Box<CreateOptions>),
}

fn parse_command(arguments: Vec<String>) -> Result<Command, CliError> {
    match arguments.as_slice() {
        [command] if command == "ping" => Ok(Command::Ping),
        [resource, action] if resource == "server" && action == "list" => Ok(Command::ServerList),
        [resource, action, id]
            if resource == "server"
                && matches!(
                    action.as_str(),
                    "get" | "status" | "instances" | "start" | "stop"
                ) =>
        {
            let server_id = id.parse().map_err(CliError::InvalidUuid)?;
            match action.as_str() {
                "get" => Ok(Command::ServerGet { server_id }),
                "status" => Ok(Command::ServerStatus { server_id }),
                "instances" => Ok(Command::ServerInstances { server_id }),
                "start" => Ok(Command::ServerStart { server_id }),
                "stop" => Ok(Command::ServerStop { server_id }),
                _ => Err(CliError::Usage),
            }
        }
        [resource, action, rest @ ..] if resource == "server" && action == "create" => {
            Ok(Command::ServerCreate(Box::new(CreateOptions::parse(rest)?)))
        }
        _ => Err(CliError::Usage),
    }
}

struct CreateOptions {
    name: String,
    repository: String,
    compute: CreateCompute,
    container_image: String,
    server_type: String,
    version: String,
    host_port: u16,
    stop_timeout_seconds: u64,
    environment: BTreeMap<String, String>,
}

enum CreateCompute {
    Local,
    Akamai {
        region: String,
        instance_type: String,
        image: String,
        firewall_id: u64,
    },
}

impl CreateOptions {
    fn parse(arguments: &[String]) -> Result<Self, CliError> {
        let mut name = None;
        let mut repository = None;
        let mut compute_provider = "local".to_owned();
        let mut akamai_region = None;
        let mut akamai_type = None;
        let mut akamai_image = None;
        let mut akamai_firewall_id = None;
        let mut container_image = DEFAULT_CONTAINER_IMAGE.to_owned();
        let mut server_type = DEFAULT_SERVER_TYPE.to_owned();
        let mut version = DEFAULT_MINECRAFT_VERSION.to_owned();
        let mut host_port = DEFAULT_HOST_PORT;
        let mut stop_timeout_seconds = DEFAULT_STOP_TIMEOUT_SECONDS;
        let mut accept_eula = false;
        let mut environment = BTreeMap::new();
        let mut index = 0;

        while index < arguments.len() {
            let flag = arguments[index].as_str();
            match flag {
                "--accept-eula" => {
                    accept_eula = true;
                    index += 1;
                }
                "--name"
                | "--repository"
                | "--compute"
                | "--akamai-region"
                | "--akamai-type"
                | "--akamai-image"
                | "--akamai-firewall-id"
                | "--image"
                | "--type"
                | "--version"
                | "--port"
                | "--stop-timeout"
                | "--env" => {
                    let value = arguments
                        .get(index + 1)
                        .ok_or_else(|| CliError::MissingFlagValueOwned(flag.to_owned()))?;
                    match flag {
                        "--name" => name = Some(value.clone()),
                        "--repository" => repository = Some(value.clone()),
                        "--compute" => compute_provider = value.clone(),
                        "--akamai-region" => akamai_region = Some(value.clone()),
                        "--akamai-type" => akamai_type = Some(value.clone()),
                        "--akamai-image" => akamai_image = Some(value.clone()),
                        "--akamai-firewall-id" => {
                            let id = value.parse().map_err(|source| CliError::InvalidInteger {
                                flag: "--akamai-firewall-id",
                                value: value.clone(),
                                source,
                            })?;
                            if id == 0 {
                                return Err(CliError::ZeroValue("--akamai-firewall-id"));
                            }
                            akamai_firewall_id = Some(id);
                        }
                        "--image" => container_image = value.clone(),
                        "--type" => server_type = value.clone(),
                        "--version" => version = value.clone(),
                        "--port" => {
                            host_port =
                                value.parse().map_err(|source| CliError::InvalidInteger {
                                    flag: "--port",
                                    value: value.clone(),
                                    source,
                                })?;
                            if host_port == 0 {
                                return Err(CliError::ZeroValue("--port"));
                            }
                        }
                        "--stop-timeout" => {
                            stop_timeout_seconds =
                                value.parse().map_err(|source| CliError::InvalidInteger {
                                    flag: "--stop-timeout",
                                    value: value.clone(),
                                    source,
                                })?;
                            if stop_timeout_seconds == 0 {
                                return Err(CliError::ZeroValue("--stop-timeout"));
                            }
                        }
                        "--env" => {
                            let (key, value) = value
                                .split_once('=')
                                .ok_or_else(|| CliError::InvalidEnvironment(value.clone()))?;
                            if key.is_empty() {
                                return Err(CliError::InvalidEnvironment(value.to_owned()));
                            }
                            environment.insert(key.to_owned(), value.to_owned());
                        }
                        _ => return Err(CliError::Usage),
                    }
                    index += 2;
                }
                _ => return Err(CliError::UnknownFlag(flag.to_owned())),
            }
        }

        if !accept_eula {
            return Err(CliError::EulaNotAccepted);
        }
        let compute = match compute_provider.as_str() {
            "local" => {
                if akamai_region.is_some()
                    || akamai_type.is_some()
                    || akamai_image.is_some()
                    || akamai_firewall_id.is_some()
                {
                    return Err(CliError::AkamaiFlagsRequireAkamaiCompute);
                }
                CreateCompute::Local
            }
            "akamai" => CreateCompute::Akamai {
                region: akamai_region.ok_or(CliError::MissingRequiredFlag("--akamai-region"))?,
                instance_type: akamai_type.ok_or(CliError::MissingRequiredFlag("--akamai-type"))?,
                image: akamai_image.ok_or(CliError::MissingRequiredFlag("--akamai-image"))?,
                firewall_id: akamai_firewall_id
                    .ok_or(CliError::MissingRequiredFlag("--akamai-firewall-id"))?,
            },
            _ => return Err(CliError::InvalidComputeProvider(compute_provider)),
        };
        Ok(Self {
            name: name.ok_or(CliError::MissingRequiredFlag("--name"))?,
            repository: repository.ok_or(CliError::MissingRequiredFlag("--repository"))?,
            compute,
            container_image,
            server_type,
            version,
            host_port,
            stop_timeout_seconds,
            environment,
        })
    }
}

#[derive(Debug, Error)]
enum CliError {
    #[error("invalid command\n\n{USAGE}")]
    Usage,
    #[error("missing value for {0}")]
    MissingFlagValue(&'static str),
    #[error("missing value for {0}")]
    MissingFlagValueOwned(String),
    #[error("missing required flag {0}")]
    MissingRequiredFlag(&'static str),
    #[error("unknown flag {0}")]
    UnknownFlag(String),
    #[error("{flag} must be an integer, got {value}")]
    InvalidInteger {
        flag: &'static str,
        value: String,
        #[source]
        source: std::num::ParseIntError,
    },
    #[error("{0} must be greater than zero")]
    ZeroValue(&'static str),
    #[error("server id is invalid")]
    InvalidUuid(#[source] uuid::Error),
    #[error("--env must use KEY=VALUE, got {0}")]
    InvalidEnvironment(String),
    #[error("--compute must be local or akamai, got {0}")]
    InvalidComputeProvider(String),
    #[error("Akamai flags require --compute akamai")]
    AkamaiFlagsRequireAkamaiCompute,
    #[error("server.create requires --accept-eula")]
    EulaNotAccepted,
    #[error("control-plane returned a server without a valid generation")]
    InvalidServerResponse,
    #[error(transparent)]
    Client(#[from] mcserver_control_plane::client::RpcClientError),
}

const USAGE: &str = r#"Usage:
  mcserverctl [--socket PATH] ping
  mcserverctl [--socket PATH] server list
  mcserverctl [--socket PATH] server get SERVER_ID
  mcserverctl [--socket PATH] server status SERVER_ID
  mcserverctl [--socket PATH] server instances SERVER_ID
  mcserverctl [--socket PATH] server start SERVER_ID
  mcserverctl [--socket PATH] server stop SERVER_ID
  mcserverctl [--socket PATH] server create \
    --name NAME --repository PATH --accept-eula \
    [--compute local|akamai] \
    [--akamai-region REGION --akamai-type TYPE --akamai-image IMAGE \
     --akamai-firewall-id ID] [--image IMAGE] [--type TYPE] \
    [--version VERSION] [--port PORT] [--stop-timeout SECONDS] \
    [--env KEY=VALUE]..."#;

#[cfg(test)]
mod tests {
    use super::{CliError, Command, CreateCompute, CreateOptions, parse_command};

    #[test]
    fn parses_status_command() {
        let command = parse_command(vec![
            "server".to_owned(),
            "status".to_owned(),
            "00000000-0000-0000-0000-000000000001".to_owned(),
        ]);
        assert!(matches!(command, Ok(Command::ServerStatus { .. })));
    }

    #[test]
    fn create_requires_a_nonzero_port() {
        let result = CreateOptions::parse(&[
            "--name".to_owned(),
            "test".to_owned(),
            "--repository".to_owned(),
            "/tmp/repository".to_owned(),
            "--accept-eula".to_owned(),
            "--port".to_owned(),
            "0".to_owned(),
        ]);
        assert!(matches!(result, Err(CliError::ZeroValue("--port"))));
    }

    #[test]
    fn parses_akamai_create_options() {
        let result = CreateOptions::parse(&[
            "--name".to_owned(),
            "remote".to_owned(),
            "--repository".to_owned(),
            "s3:s3.example.invalid/bucket".to_owned(),
            "--accept-eula".to_owned(),
            "--compute".to_owned(),
            "akamai".to_owned(),
            "--akamai-region".to_owned(),
            "jp-tyo-3".to_owned(),
            "--akamai-type".to_owned(),
            "g6-nanode-1".to_owned(),
            "--akamai-image".to_owned(),
            "linode/debian13".to_owned(),
            "--akamai-firewall-id".to_owned(),
            "123".to_owned(),
        ]);

        assert!(matches!(
            result,
            Ok(CreateOptions {
                compute: CreateCompute::Akamai {
                    firewall_id: 123,
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn requires_akamai_firewall_id() {
        let result = CreateOptions::parse(&[
            "--name".to_owned(),
            "remote".to_owned(),
            "--repository".to_owned(),
            "s3:s3.example.invalid/bucket".to_owned(),
            "--accept-eula".to_owned(),
            "--compute".to_owned(),
            "akamai".to_owned(),
            "--akamai-region".to_owned(),
            "jp-tyo-3".to_owned(),
            "--akamai-type".to_owned(),
            "g6-nanode-1".to_owned(),
            "--akamai-image".to_owned(),
            "linode/debian13".to_owned(),
        ]);

        assert!(matches!(
            result,
            Err(CliError::MissingRequiredFlag("--akamai-firewall-id"))
        ));
    }

    #[test]
    fn rejects_akamai_flags_for_local_compute() {
        let result = CreateOptions::parse(&[
            "--name".to_owned(),
            "local".to_owned(),
            "--repository".to_owned(),
            "/tmp/repository".to_owned(),
            "--accept-eula".to_owned(),
            "--akamai-region".to_owned(),
            "jp-tyo-3".to_owned(),
        ]);

        assert!(matches!(
            result,
            Err(CliError::AkamaiFlagsRequireAkamaiCompute)
        ));
    }
}
