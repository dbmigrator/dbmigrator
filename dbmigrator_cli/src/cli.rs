//! Defines the CLI application

use dbmigrator::MigratorError;
use dbmigrator::RecipeError;
use std::path::PathBuf;
use thiserror::Error;

#[derive(clap::Parser, Debug)]
#[command(version, about)]
pub struct Cli {
    /// Database URL
    #[arg(short = 'D', long)]
    pub db_url: Option<String>,

    /// Migration recipes directory path
    #[arg(short = 'M', long, default_value = "./migrations")]
    pub migrations: PathBuf,
    
    /// DDL dump directory path
    #[arg(long, default_value = "./ddl")]
    pub ddl_path: PathBuf,

    /// Allow creating changelog table if not exists.
    #[arg(long, default_value = "false")]
    pub auto_initialize: bool,

    /// Set changelog table name
    #[arg(long, default_value = "dbmigrator_log")]
    pub changelog_table_name: String,

    /// Baseline for initialization (if not defined use last available baseline).
    #[arg(long)]
    pub suggested_baseline_version: Option<String>,

    /// Limit migration to the specified version (if not defined apply all).
    #[arg(long)]
    pub target_version: Option<String>,

    /// Allow applying pending revert and fixup migrations
    #[arg(long, default_value = "false")]
    pub allow_fixes: bool,

    /// Allow to out of order migrations
    #[arg(long, default_value = "false")]
    pub allow_out_of_order: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Create empty DB and required DB roles.
    CreateDB,

    /// Dump schema backup and compare with baseline
    DumpSchema,

    /// Main migrate operation
    Migrate(MigrateArgs),

    /// Show loaded configuration and recipies
    ShowConfig,

    /// Display log of applied migrations
    ShowChangelog(ShowChangelogArgs),

    /// Display pending migration plan
    ShowPlan,

    /// Check the overall status of DB schema and pending migrations
    ///
    /// The current status is printed on stdout.
    /// Returns exit code 0 for `up-to-date`, or non-zero otherwise.
    Status(StatusArgs),
}

#[derive(clap::Args, Debug, Copy, Clone)]
pub struct ShowChangelogArgs {
    /// Show changelog with effective migrations (without reverted recipes and after fixups)
    #[arg(short = 'c', long, default_value = "false")]
    pub consolidated: bool,

    /// Show consolidated changelog including pending migrations
    #[arg(short = 'p', long, default_value = "false")]
    pub with_pending: bool,
}

#[derive(clap::Args, Debug, Copy, Clone)]
pub struct StatusArgs {
    /// Suppress output on stdout
    #[arg(short = 'q', long, default_value = "false")]
    pub quiet: bool,
}

#[derive(clap::Args, Debug, Copy, Clone)]
pub struct MigrateArgs {
    /// Commit pending changes to the database
    #[arg(short = 'C', long, default_value = "false")]
    pub commit: bool,
}

/// An Error occurred during a migration cycle
#[derive(Debug, Error)]
pub enum CliError {
    #[error("unknown command")]
    UnknownCommand,

    #[error("not implemented")]
    NotImplemented,

    #[error(transparent)]
    IoError(std::io::Error),

    #[error(transparent)]
    MigratorError(MigratorError),

    #[error(transparent)]
    TimeError(time::Error),
}

impl From<MigratorError> for CliError {
    fn from(err: MigratorError) -> CliError {
        CliError::MigratorError(err)
    }
}

impl From<RecipeError> for CliError {
    fn from(err: RecipeError) -> CliError {
        CliError::MigratorError(MigratorError::RecipeError(err))
    }
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> CliError {
        CliError::IoError(err)
    }
}

impl From<time::Error> for CliError {
    fn from(err: time::Error) -> CliError {
        CliError::TimeError(err)
    }
}

impl From<time::error::InvalidFormatDescription> for CliError {
    fn from(err: time::error::InvalidFormatDescription) -> CliError {
        CliError::TimeError(time::Error::InvalidFormatDescription(err))
    }
}

impl From<time::error::Format> for CliError {
    fn from(err: time::error::Format) -> CliError {
        CliError::TimeError(time::Error::Format(err))
    }
}
