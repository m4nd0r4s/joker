//! `jk`, the Joker command line (docs/07).

mod apps;
mod board;
mod class;
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
    /// Compile the apps to Wasm and, with `--target`, precompile them for a node.
    Build {
        /// A board (`rpi3b-plus@1`) or a target triple (`aarch64-unknown-linux-gnu`).
        #[arg(long)]
        target: Option<String>,
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
    /// A live view of the runtime's own metrics, redrawn once a second.
    Top,
    /// Read signals with quality and age.
    Read {
        /// Selectors (`pi.**`, `inv*.ac.p`).
        #[arg(required = true)]
        selectors: Vec<String>,
        /// Keep printing updates over the node's live subscription
        /// (`GET /v1/ws`, ref/09 §6), instead of one read.
        #[arg(long)]
        watch: bool,
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
    /// Ends a trip on an output (docs/16 E7).
    Reset {
        /// The output.
        output: String,
        /// Why.
        #[arg(long)]
        reason: String,
    },
    /// Lists tripped outputs (docs/16 E7).
    Trips,
    /// Calls a class method as a manual writer (docs/17 §4.1).
    Call {
        /// `<instance>.<method>`.
        target_method: String,
        /// Arguments, as `name=value` (bool/number/quantity).
        args: Vec<String>,
        /// How long the writes hold (mandatory).
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
    /// The node's structured logs, oldest first (ref/09 §5.2).
    Logs {
        /// Only this app's lines (`app:<a>`).
        #[arg(long, conflicts_with = "source")]
        app: Option<String>,
        /// Only this source: `jokerd`, `driver:<d>`, `app:<a>`, `api`.
        #[arg(long)]
        source: Option<String>,
        /// Only this level and above: trace, debug, info, warn, error.
        #[arg(long)]
        level: Option<String>,
        /// Only the last duration (`10m`, `1h`).
        #[arg(long)]
        since: Option<String>,
        /// How many lines.
        #[arg(long, default_value_t = 100)]
        limit: u32,
        /// Keep printing new lines as they arrive (`GET /v1/ws`, topic
        /// `logs`).
        #[arg(long)]
        follow: bool,
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
    /// Queries the historian (ref/09 §5.5).
    History {
        /// Selectors.
        #[arg(required = true)]
        selectors: Vec<String>,
        /// Start: RFC 3339, `now` or `-<duration>`; default one hour before `--to`.
        #[arg(long, allow_hyphen_values = true)]
        from: Option<String>,
        /// End; default now.
        #[arg(long, allow_hyphen_values = true)]
        to: Option<String>,
        /// Aggregation interval (`1m`); without it, raw points.
        #[arg(long)]
        step: Option<String>,
        /// Aggregates (`avg,min,max,count,quality` by default with `--step`).
        #[arg(long)]
        agg: Option<String>,
        /// `auto`, `raw`, `1s`, `1m` or `15m`.
        #[arg(long)]
        tier: Option<String>,
    },
    /// Lists the alarms, then the latest transitions (ref/09 §5.8).
    Alarms {
        /// Only the active ones.
        #[arg(long)]
        active: bool,
    },
    /// Acts on one alarm (ref/09 §5.8).
    Alarm {
        #[command(subcommand)]
        command: AlarmCommand,
    },
    /// Operates an app on the node (ref/09 §5.14, ADR 0091).
    App {
        #[command(subcommand)]
        command: AppCommand,
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
    /// Equipment classes (docs/14, docs/17).
    Class {
        #[command(subcommand)]
        command: ClassCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ClassCommand {
    /// The available classes, with versions.
    List {
        /// Only the classes of `std/`.
        #[arg(long, conflicts_with = "project")]
        std: bool,
        /// Only the classes of the project's `classes/`.
        #[arg(long)]
        project: bool,
    },
    /// Members, capabilities, methods, queries and reference standards of a class.
    Show {
        /// The class (`std/led@1`; `std/` may be omitted).
        class: String,
    },
    /// Validates the classes of `std/` and of the project (JC codes).
    Check,
}

#[derive(Debug, Subcommand)]
enum AppCommand {
    /// The node's apps and their state.
    List,
    /// Lets a stopped app run again.
    Start {
        /// The app.
        app: String,
    },
    /// Stops an app: its tasks skip their ticks and its outputs follow
    /// their leases, until a start or the next generation.
    Stop {
        /// The app.
        app: String,
        /// Why.
        reason: String,
    },
    /// Swaps fresh instances of an app's cycle tasks in.
    Restart {
        /// The app.
        app: String,
        /// Why.
        #[arg(long)]
        reason: Option<String>,
    },
    /// The app's log lines (`jk logs --app <app>`).
    Logs {
        /// The app.
        app: String,
        /// Only this level and above: trace, debug, info, warn, error.
        #[arg(long)]
        level: Option<String>,
        /// How many lines.
        #[arg(long, default_value_t = 100)]
        limit: u32,
        /// Keep printing new lines as they arrive.
        #[arg(long)]
        follow: bool,
    },
}

#[derive(Debug, Subcommand)]
enum AlarmCommand {
    /// Acknowledges an alarm.
    Ack {
        /// The alarm id.
        id: String,
        /// Why.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Shelves an alarm for a TTL, at most its `max_shelve`.
    Shelve {
        /// The alarm id.
        id: String,
        /// How long.
        ttl: String,
        /// Why.
        reason: String,
    },
    /// Ends the shelve of an alarm.
    Unshelve {
        /// The alarm id.
        id: String,
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
    /// Renders a board profile's declared peripherals (local checks only;
    /// no `--node` yet).
    Show {
        /// The board profile file.
        file: PathBuf,
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
        Command::Build { target } => apps::build_command(&entry()?, target.as_deref(), cli.json),
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
        Command::Top => node()?.top(),
        Command::Read { selectors, watch } => {
            if watch {
                node()?.read_watch(&selectors)
            } else {
                node()?.read(&selectors)
            }
        }
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
        Command::Reset { output, reason } => node()?.reset(&output, &reason),
        Command::Trips => node()?.trips(),
        Command::Action {
            target_action,
            args,
            key,
        } => node()?.action(&target_action, &args, &key),
        Command::Call {
            target_method,
            args,
            ttl,
            reason,
            level,
            takeover,
        } => node()?.call(
            &target_method,
            &args,
            &ttl,
            &reason,
            level.as_deref(),
            takeover,
        ),
        Command::Decisions { output, limit } => node()?.decisions(&output, limit),
        Command::Logs {
            app,
            source,
            level,
            since,
            limit,
            follow,
        } => node()?.logs(
            &[
                ("app", app),
                ("source", source),
                ("level", level),
                ("since", since),
                ("limit", Some(limit.to_string())),
            ],
            follow,
        ),
        Command::History {
            selectors,
            from,
            to,
            step,
            agg,
            tier,
        } => node()?.history(&node::HistoryArgs {
            selectors,
            from,
            to,
            step,
            agg,
            tier,
        }),
        Command::Alarms { active } => node()?.alarms(active),
        Command::Alarm { command } => match command {
            AlarmCommand::Ack { id, reason } => node()?.alarm_ack(&id, reason.as_deref()),
            AlarmCommand::Shelve { id, ttl, reason } => node()?.alarm_shelve(&id, &ttl, &reason),
            AlarmCommand::Unshelve { id } => node()?.alarm_unshelve(&id),
        },
        Command::App { command } => match command {
            AppCommand::List => node()?.apps(),
            AppCommand::Start { app } => node()?.app_lifecycle(&app, "start", None),
            AppCommand::Stop { app, reason } => node()?.app_lifecycle(&app, "stop", Some(&reason)),
            AppCommand::Restart { app, reason } => {
                node()?.app_lifecycle(&app, "restart", reason.as_deref())
            }
            AppCommand::Logs {
                app,
                level,
                limit,
                follow,
            } => node()?.logs(
                &[
                    ("app", Some(app)),
                    ("level", level),
                    ("limit", Some(limit.to_string())),
                ],
                follow,
            ),
        },
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
        Command::Board {
            command: BoardCommand::Show { file },
        } => board::show(&file),
        Command::Class { command } => {
            let entry = entry()?;
            let root = entry
                .manifest
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map_or_else(|| PathBuf::from("."), PathBuf::from);
            match command {
                ClassCommand::List { std, project } => class::list(&root, std, project),
                ClassCommand::Show { class } => class::show(&root, &class),
                ClassCommand::Check => class::check(&root),
            }
        }
    }
}
