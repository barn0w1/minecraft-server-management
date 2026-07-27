use std::{env, error::Error, path::PathBuf};

use mcserver_control_plane::client::UnixRpcClient;
use mcserver_protocol::client::{ComputeSpec, DesiredDataSpec, ProcessSpec, method};
use serde::Deserialize;
use serde_json::{Value, json};
use thiserror::Error;

const DEFAULT_SOCKET_PATH: &str = "/run/mcserver/control-plane.sock";
const MAX_DEFINITION_BYTES: usize = 1024 * 1024;

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
        Command::Ping => call(client, method::SYSTEM_PING, Value::Null).await,
        Command::ServerList { include_archived } => {
            call(
                client,
                method::SERVER_LIST,
                json!({ "include_archived": include_archived }),
            )
            .await
        }
        Command::ServerGet { server_name } => {
            call_server_name(client, method::SERVER_GET, &server_name).await
        }
        Command::ServerStatus { server_name } => {
            call_server_name(client, method::SERVER_STATUS, &server_name).await
        }
        Command::ServerInstances { server_name } => {
            call_server_name(client, method::SERVER_INSTANCE_LIST, &server_name).await
        }
        Command::ServerStart { server_name } => {
            set_desired_state(client, &server_name, "running").await
        }
        Command::ServerStop { server_name } => {
            set_desired_state(client, &server_name, "stopped").await
        }
        Command::ServerArchive { server_name } => archive(client, &server_name).await,
        Command::ServerCreate(options) => {
            apply_definition(client, method::SERVER_CREATE, options, None).await
        }
        Command::ServerApply {
            options,
            expected_generation,
        } => apply_definition(client, method::SERVER_APPLY, options, expected_generation).await,
    }
}

async fn apply_definition(
    client: &UnixRpcClient,
    method_name: &str,
    options: DefinitionOptions,
    expected_generation: Option<u64>,
) -> Result<Value, CliError> {
    let definition = ServerDefinition::load(&options.file)?;
    let mut params = json!({
        "name": definition.name,
        "spec": {
            "compute": definition.compute,
            "process": definition.process,
            "data": definition.data,
        },
    });
    if let Some(generation) = expected_generation {
        params["expected_generation"] = generation.into();
    }
    let server = call(client, method_name, params).await?;
    if !options.start {
        return Ok(server);
    }
    let name = server
        .get("name")
        .and_then(Value::as_str)
        .ok_or(CliError::InvalidServerResponse)?
        .to_owned();
    set_desired_state_from_resource(client, &name, server, "running").await
}

async fn call(client: &UnixRpcClient, method_name: &str, params: Value) -> Result<Value, CliError> {
    client
        .call_value(method_name, params)
        .await
        .map_err(Into::into)
}

async fn call_server_name(
    client: &UnixRpcClient,
    method_name: &str,
    server_name: &str,
) -> Result<Value, CliError> {
    call(client, method_name, json!({ "server_name": server_name })).await
}

async fn set_desired_state(
    client: &UnixRpcClient,
    server_name: &str,
    desired_state: &str,
) -> Result<Value, CliError> {
    let server = call_server_name(client, method::SERVER_GET, server_name).await?;
    set_desired_state_from_resource(client, server_name, server, desired_state).await
}

async fn set_desired_state_from_resource(
    client: &UnixRpcClient,
    server_name: &str,
    server: Value,
    desired_state: &str,
) -> Result<Value, CliError> {
    let generation = server
        .get("generation")
        .and_then(Value::as_u64)
        .ok_or(CliError::InvalidServerResponse)?;
    call(
        client,
        method::SERVER_SET_DESIRED_STATE,
        json!({
            "server_name": server_name,
            "desired_state": desired_state,
            "expected_generation": generation,
        }),
    )
    .await
}

async fn archive(client: &UnixRpcClient, server_name: &str) -> Result<Value, CliError> {
    let server = call_server_name(client, method::SERVER_GET, server_name).await?;
    let generation = server
        .get("generation")
        .and_then(Value::as_u64)
        .ok_or(CliError::InvalidServerResponse)?;
    call(
        client,
        method::SERVER_ARCHIVE,
        json!({
            "server_name": server_name,
            "expected_generation": generation,
        }),
    )
    .await
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
        Ok(Self {
            socket_path,
            command: parse_command(positional)?,
        })
    }
}

enum Command {
    Ping,
    ServerList {
        include_archived: bool,
    },
    ServerGet {
        server_name: String,
    },
    ServerStatus {
        server_name: String,
    },
    ServerInstances {
        server_name: String,
    },
    ServerStart {
        server_name: String,
    },
    ServerStop {
        server_name: String,
    },
    ServerArchive {
        server_name: String,
    },
    ServerCreate(DefinitionOptions),
    ServerApply {
        options: DefinitionOptions,
        expected_generation: Option<u64>,
    },
}

fn parse_command(arguments: Vec<String>) -> Result<Command, CliError> {
    match arguments.as_slice() {
        [command] if command == "ping" => Ok(Command::Ping),
        [resource, action] if resource == "server" && action == "list" => Ok(Command::ServerList {
            include_archived: false,
        }),
        [resource, action, flag]
            if resource == "server" && action == "list" && flag == "--include-archived" =>
        {
            Ok(Command::ServerList {
                include_archived: true,
            })
        }
        [resource, action, name]
            if resource == "server"
                && matches!(
                    action.as_str(),
                    "get" | "status" | "instances" | "start" | "stop" | "archive"
                ) =>
        {
            match action.as_str() {
                "get" => Ok(Command::ServerGet {
                    server_name: name.clone(),
                }),
                "status" => Ok(Command::ServerStatus {
                    server_name: name.clone(),
                }),
                "instances" => Ok(Command::ServerInstances {
                    server_name: name.clone(),
                }),
                "start" => Ok(Command::ServerStart {
                    server_name: name.clone(),
                }),
                "stop" => Ok(Command::ServerStop {
                    server_name: name.clone(),
                }),
                "archive" => Ok(Command::ServerArchive {
                    server_name: name.clone(),
                }),
                _ => Err(CliError::Usage),
            }
        }
        [resource, action, rest @ ..] if resource == "server" && action == "create" => {
            Ok(Command::ServerCreate(DefinitionOptions::parse(rest)?))
        }
        [resource, action, rest @ ..] if resource == "server" && action == "apply" => {
            let (options, expected_generation) = DefinitionOptions::parse_apply(rest)?;
            Ok(Command::ServerApply {
                options,
                expected_generation,
            })
        }
        _ => Err(CliError::Usage),
    }
}

struct DefinitionOptions {
    file: PathBuf,
    start: bool,
}

impl DefinitionOptions {
    fn parse(arguments: &[String]) -> Result<Self, CliError> {
        let (options, expected_generation) = Self::parse_apply(arguments)?;
        if expected_generation.is_some() {
            return Err(CliError::CreateExpectedGeneration);
        }
        Ok(options)
    }

    fn parse_apply(arguments: &[String]) -> Result<(Self, Option<u64>), CliError> {
        let mut file = None;
        let mut start = false;
        let mut expected_generation = None;
        let mut index = 0;
        while index < arguments.len() {
            match arguments[index].as_str() {
                "--start" => {
                    start = true;
                    index += 1;
                }
                "--file" | "--expected-generation" => {
                    let flag = arguments[index].as_str();
                    let value = arguments
                        .get(index + 1)
                        .ok_or_else(|| CliError::MissingFlagValueOwned(flag.to_owned()))?;
                    if flag == "--file" {
                        file = Some(PathBuf::from(value));
                    } else {
                        let generation =
                            value.parse().map_err(|source| CliError::InvalidInteger {
                                flag: "--expected-generation",
                                value: value.clone(),
                                source,
                            })?;
                        if generation == 0 {
                            return Err(CliError::ZeroValue("--expected-generation"));
                        }
                        expected_generation = Some(generation);
                    }
                    index += 2;
                }
                flag => return Err(CliError::UnknownFlag(flag.to_owned())),
            }
        }
        Ok((
            Self {
                file: file.ok_or(CliError::MissingRequiredFlag("--file"))?,
                start,
            },
            expected_generation,
        ))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServerDefinition {
    schema_version: u32,
    name: String,
    compute: ComputeSpec,
    process: ProcessSpec,
    data: DesiredDataSpec,
}

impl ServerDefinition {
    fn load(path: &PathBuf) -> Result<Self, CliError> {
        let bytes = std::fs::read(path).map_err(|source| CliError::DefinitionIo {
            path: path.clone(),
            source,
        })?;
        if bytes.len() > MAX_DEFINITION_BYTES {
            return Err(CliError::DefinitionTooLarge {
                actual: bytes.len(),
                maximum: MAX_DEFINITION_BYTES,
            });
        }
        let text = std::str::from_utf8(&bytes).map_err(CliError::DefinitionEncoding)?;
        let definition: Self = toml::from_str(text)?;
        if definition.schema_version != 1 {
            return Err(CliError::UnsupportedDefinitionSchema(
                definition.schema_version,
            ));
        }
        Ok(definition)
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
    #[error("--expected-generation is only valid with server apply")]
    CreateExpectedGeneration,
    #[error("server definition {path} could not be read")]
    DefinitionIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("server definition is not UTF-8")]
    DefinitionEncoding(#[source] std::str::Utf8Error),
    #[error("server definition is invalid TOML")]
    DefinitionToml(#[from] toml::de::Error),
    #[error("server definition is {actual} bytes; maximum is {maximum}")]
    DefinitionTooLarge { actual: usize, maximum: usize },
    #[error("server definition schema_version {0} is unsupported; expected 1")]
    UnsupportedDefinitionSchema(u32),
    #[error("control-plane returned an invalid server resource")]
    InvalidServerResponse,
    #[error(transparent)]
    Client(#[from] mcserver_control_plane::client::RpcClientError),
}

const USAGE: &str = r#"Usage:
  mcserverctl [--socket PATH] ping
  mcserverctl [--socket PATH] server list [--include-archived]
  mcserverctl [--socket PATH] server get SERVER_NAME
  mcserverctl [--socket PATH] server status SERVER_NAME
  mcserverctl [--socket PATH] server instances SERVER_NAME
  mcserverctl [--socket PATH] server start SERVER_NAME
  mcserverctl [--socket PATH] server stop SERVER_NAME
  mcserverctl [--socket PATH] server archive SERVER_NAME
  mcserverctl [--socket PATH] server create --file FILE [--start]
  mcserverctl [--socket PATH] server apply --file FILE [--start]
    [--expected-generation GENERATION]"#;

#[cfg(test)]
mod tests {
    use super::{CliError, Command, DefinitionOptions, ServerDefinition, parse_command};
    use std::path::PathBuf;

    #[test]
    fn parses_status_command() {
        let command = parse_command(vec![
            "server".to_owned(),
            "status".to_owned(),
            "community".to_owned(),
        ]);
        assert!(matches!(
            command,
            Ok(Command::ServerStatus { server_name }) if server_name == "community"
        ));
    }

    #[test]
    fn create_requires_a_definition_file() {
        assert!(matches!(
            DefinitionOptions::parse(&[]),
            Err(CliError::MissingRequiredFlag("--file"))
        ));
    }

    #[test]
    fn parses_r2_server_definition() -> Result<(), Box<dyn std::error::Error>> {
        let definition: ServerDefinition = toml::from_str(
            r#"
schema_version = 1
name = "community"

[compute]
provider = "akamai"
region = "jp-tyo-3"
instance_type = "g6-nanode-1"
image = "linode/debian13"
firewall_id = 123

[process]
container_image = "docker.io/itzg/minecraft-server:latest"
server_type = "VANILLA"
version = "LATEST"
host_port = 25565
stop_timeout_seconds = 60
accept_eula = true

[process.environment]
MEMORY = "1G"

[data]
backend = "r2_restic"
"#,
        )?;
        assert_eq!(definition.schema_version, 1);
        assert_eq!(definition.name, "community");
        Ok(())
    }

    #[test]
    fn definition_options_accept_start() -> Result<(), CliError> {
        let options = DefinitionOptions::parse(&[
            "--file".to_owned(),
            "server.toml".to_owned(),
            "--start".to_owned(),
        ])?;
        assert_eq!(options.file, PathBuf::from("server.toml"));
        assert!(options.start);
        Ok(())
    }
}
