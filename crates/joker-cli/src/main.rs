//! `jk`, the Joker command line (docs/07).

mod apps;
mod board;
mod dev;
mod node;
mod profile;
mod project;
mod replay;

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};

/// The Joker command line.
#[derive(Debug, Parser)]
#[command(name = "jk", version, about = "The Joker command line")]
struct Cli {
    /// The node: `local`, a context of `[nodes]`, `unix:<path>` or `ssh://[user@]host[:port]`.
    #[arg(long, global = true, env = "JK_NODE")]
    node: Option<String>,
    /// The entry overlay by variant (`--env lab` is `joker.lab.toml`).
    #[arg(long, global = true, env = "JK_ENV")]
    env: Option<String>,
    /// The entry manifest.
    #[arg(long, global = true, env = "JK_MANIFEST")]
    manifest: Option<PathBuf>,
    /// Print the API's JSON instead of the human output.
    #[arg(long, global = true)]
    json: bool,
    /// How long to wait for the node.
    #[arg(long, global = true, env = "JK_TIMEOUT", default_value = "30s")]
    timeout: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a project in a new folder.
    New {
        /// The project name (and folder).
        name: String,
    },
    /// Check the manifest, its bindings, units and envelopes.
    Check,
    /// Run the manifest on a local runtime with simulated devices, reloading on every save.
    Dev {
        /// The socket of the local runtime.
        #[arg(long, default_value = ".joker/dev.sock")]
        socket: PathBuf,
    },
    /// Send the manifest to the node as a new generation and activate it.
    Apply {
        /// Confirmation window: without `jk confirm` in time, the node rolls back.
        #[arg(long)]
        confirm: Option<String>,
        /// Validate and plan only.
        #[arg(long)]
        dry_run: bool,
        /// A message for the generation.
        #[arg(long, short)]
        message: Option<String>,
    },
    /// Confirm the generation waiting for confirmation.
    Confirm,
    /// Reactivate a previous generation.
    Rollback {
        /// The generation; default the previous one.
        generation: Option<String>,
        /// Why.
        #[arg(long, default_value = "jk rollback")]
        reason: String,
    },
    /// The generations of the node.
    Generations,
    /// A summary of the node.
    Status,
    /// Read signals with quality and age.
    Read {
        /// Selectors (`pi.**`, `inv*.ac.p`).
        #[arg(required = true)]
        selectors: Vec<String>,
    },
    /// Write an output as a person, at a manual level, for a while.
    Write {
        /// The output.
        output: String,
        /// The value (a number in the output's unit, a quantity such as `50 kW`, `true`, a variant).
        value: String,
        /// How long the write holds (mandatory).
        #[arg(long)]
        ttl: String,
        /// Why (mandatory).
        #[arg(long)]
        reason: String,
        /// The manual level, default `manual:4`.
        #[arg(long)]
        level: Option<String>,
        /// Take the level from another person.
        #[arg(long)]
        takeover: bool,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Release a manual level.
    Release {
        /// The output.
        output: String,
        /// The level.
        #[arg(long)]
        level: Option<String>,
        /// Release another person's level.
        #[arg(long)]
        force: bool,
    },
    /// Record a reproducible trace, starting with the state of every app.
    Record {
        /// How long to record.
        #[arg(long = "for")]
        duration: String,
        /// Only these groups.
        #[arg(long)]
        groups: Vec<String>,
        /// The file to write.
        #[arg(long)]
        out: PathBuf,
    },
    /// Cut the continuous trace ring (no app state: the replay starts from `init`).
    TraceDump {
        /// The last part of the ring.
        #[arg(long)]
        last: Option<String>,
        /// The file to write.
        #[arg(long)]
        out: PathBuf,
    },
    /// Replay a trace on this machine with the project's apps, from the recorded state.
    Replay {
        /// The trace.
        file: PathBuf,
        /// Show the ticks that differ.
        #[arg(long)]
        diff: bool,
    },
    /// The arbiter state of an output: owning group, levels, active
    /// level, last result (docs/07; `jk explain <key|code|pipeline>` is
    /// not built yet — this only reads an output path).
    Explain {
        /// The output path.
        target: String,
    },
    /// Invokes an action of a device (ref/09 §5.10).
    Action {
        /// `<target>.<action>`.
        target_action: String,
        /// Arguments, as `name=value` (bool/number/string).
        args: Vec<String>,
        /// Idempotency key: a reinvocation with the same key, within the
        /// action's `dedup_window`, replays its earlier result.
        #[arg(long)]
        key: String,
    },
    /// The decision log.
    Decisions {
        /// Only these outputs (selectors).
        #[arg(long)]
        output: Vec<String>,
        /// How many entries.
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Profile draft tooling (ref/03 §2.2.19).
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Board profile tooling (ref/03 §2.3, docs/07).
    Board {
        #[command(subcommand)]
        command: BoardCommand,
    },
}

#[derive(Debug, Subcommand)]
enum BoardCommand {
    /// Validates a board profile (local checks only; no `--node` yet).
    Check {
        /// The board profile file.
        file: PathBuf,
    },
    /// Copies an existing board profile to a new name.
    New {
        /// The new board's name.
        name: String,
        /// The board profile to copy, as a path or a `boards/<name>.board.toml` name.
        #[arg(long)]
        from: String,
    },
}

#[derive(Debug, Subcommand)]
enum ProfileCommand {
    /// Turns a register map into a profile draft.
    Import {
        #[command(subcommand)]
        format: ImportFormat,
    },
}

#[derive(Debug, Subcommand)]
enum ImportFormat {
    /// From a Modbus register-map CSV (ref/03 §2.2.19).
    Csv {
        /// The CSV file, or `-` for stdin.
        file: String,
        /// The profile name (`<vendor>/<model>`).
        #[arg(long)]
        name: String,
        /// The driver family; only `modbus` in 1.0.
        #[arg(long)]
        driver: String,
        /// Where to write the draft; defaults to `profiles/<name>@1.profile.toml`.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Reads `at` in the base-1 ("Modicon") notation.
        #[arg(long)]
        modicon: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("{}", e.message);
            ExitCode::from(e.exit)
        }
    }
}

/// An error and its exit code (docs/07 §8).
#[derive(Debug)]
pub struct Failure {
    /// The message.
    pub message: String,
    /// The exit code.
    pub exit: u8,
}

impl Failure {
    /// A usage error (exit 1).
    pub fn usage(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit: 1,
        }
    }
}

impl From<joker_client::ClientError> for Failure {
    fn from(e: joker_client::ClientError) -> Self {
        Self {
            message: e.message,
            exit: e.exit,
        }
    }
}

fn timeout(text: &str) -> Result<Duration, Failure> {
    jokerd::wire::parse_duration(text)
        .map(Duration::from_nanos)
        .map_err(|e| Failure::usage(format!("--timeout {e}")))
}

fn run(cli: Cli) -> Result<u8, Failure> {
    let entry =
        || project::entry(cli.env.as_deref(), cli.manifest.as_deref()).map_err(Failure::usage);
    let timeout = timeout(&cli.timeout)?;
    let node = || node::Node::open(cli.node.as_deref(), timeout, cli.json);
    match cli.command {
        Command::New { name } => {
            project::new_project(&PathBuf::from(&name), &name).map_err(Failure::usage)?;
            println!("created {name}/");
            println!("  cd {name}\n  jk dev --env sim");
            Ok(0)
        }
        Command::Check => node::check(&entry()?, cli.json),
        Command::Dev { socket } => dev::run(&entry()?, &socket),
        Command::Apply {
            confirm,
            dry_run,
            message,
        } => node()?.apply(&entry()?, confirm.as_deref(), dry_run, message),
        Command::Confirm => node()?.confirm(),
        Command::Rollback { generation, reason } => {
            node()?.rollback(generation.as_deref(), &reason)
        }
        Command::Generations => node()?.generations(),
        Command::Status => node()?.status(),
        Command::Read { selectors } => node()?.read(&selectors),
        Command::Write {
            output,
            value,
            ttl,
            reason,
            level,
            takeover,
            yes,
        } => node()?.write(
            &output,
            &value,
            &ttl,
            &reason,
            level.as_deref(),
            takeover,
            yes,
        ),
        Command::Release {
            output,
            level,
            force,
        } => node()?.release(&output, level.as_deref(), force),
        Command::Explain { target } => node()?.explain(&target),
        Command::Action {
            target_action,
            args,
            key,
        } => node()?.action(&target_action, &args, &key),
        Command::Decisions { output, limit } => node()?.decisions(&output, limit),
        Command::Record {
            duration,
            groups,
            out,
        } => node()?.record(&duration, &groups, &out),
        Command::TraceDump { last, out } => node()?.trace_dump(last.as_deref(), &out),
        Command::Replay { file, diff } => replay::run(&entry()?, &file, diff, cli.json),
        Command::Profile {
            command:
                ProfileCommand::Import {
                    format:
                        ImportFormat::Csv {
                            file,
                            name,
                            driver,
                            out,
                            modicon,
                        },
                },
        } => profile::import_csv_command(&file, &name, &driver, out.as_deref(), modicon),
        Command::Board {
            command: BoardCommand::Check { file },
        } => board::check(&file),
        Command::Board {
            command: BoardCommand::New { name, from },
        } => board::new(&name, &from),
    }
}
