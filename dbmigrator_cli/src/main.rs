//! Main entry point for the dbmigrator cli tool

mod cli;

use crate::cli::{CliError, Command};
use clap::Parser;
use cli::Cli;
use comfy_table::{Cell, CellAlignment, Table};
use console::{Style, Term};
use dbmigrator::{
    simple_compare, simple_kind_detector, AsyncDriver, Changelog, Config, Migrator,
    SIMPLE_FILENAME_PATTERN,
};
use indicatif::{HumanDuration, ProgressBar, ProgressStyle};
use std::time::Instant;
use time::ext::NumericalDuration;

fn main() {
    human_panic::setup_panic!(human_panic::Metadata::new(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    )
    .homepage(env!("CARGO_PKG_HOMEPAGE"))
    .support("Open a issue at https://github.com/dbmigrator/dbmigrator/issue"));

    if let Err(e) = crate::inner_main() {
        eprintln!("{e}");
        std::process::exit(1)
    }
}

fn inner_main() -> Result<(), CliError> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::ShowConfig) | Some(Command::ShowChangelog(_)) | Some(Command::ShowPlan) => {
            migrator_command(&cli)
        }
        Some(Command::Status(_)) => match migrator_command(&cli) {
            Ok(_) => Ok(()),
            Err(e) => {
                println!(
                    "{}",
                    match e {
                        CliError::IoError(_) => "io-error",
                        CliError::MigratorError(e) => match e {
                            dbmigrator::MigratorError::NoLogTable() => "db-uninitialized",
                            dbmigrator::MigratorError::PgError(_) => "db-error",
                            dbmigrator::MigratorError::RecipeError(_) => "recipe-error",
                            _ => "internal-error",
                        },
                        _ => "internal-error",
                    }
                );
                std::process::exit(1)
            }
        },
        Some(Command::Migrate(_)) => migrator_command(&cli),
        _ => Err(CliError::UnknownCommand),
    }
}

fn show_config(migrator: &Migrator) {
    let mut table = Table::new();
    table
        .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
        .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
        .set_header(vec!["Version", "Name", "Kind", "Checksum"]);
    for script in migrator.recipes() {
        table.add_row(vec![
            Cell::new(if let Some(new_version) = script.new_version() {
                if script.version() != new_version {
                    format!("{} -> {}", script.version(), new_version)
                } else {
                    script.version().to_string()
                }
            } else {
                script.version().to_string()
            }),
            Cell::new(script.name()),
            Cell::new(script.kind().to_string()).fg(match script.kind() {
                dbmigrator::RecipeKind::Baseline => comfy_table::Color::Cyan,
                dbmigrator::RecipeKind::Upgrade => comfy_table::Color::Green,
                dbmigrator::RecipeKind::Fixup => comfy_table::Color::Yellow,
                dbmigrator::RecipeKind::Revert => comfy_table::Color::Red,
            }),
            Cell::new(match (script.old_checksum32(), script.new_checksum32()) {
                (Some(old), Some(new)) => format!("{} -> {}", old, new),
                (Some(old), None) => format!("{} -> revert", old),
                (_, _) => script.checksum32().to_string(),
            }),
        ]);
    }
    println!("Migration scripts:\n{table}");
}

fn show_plan(migrator: &Migrator) {
    if migrator.plans().is_empty() {
        println!("No pending migrations.");
    } else {
        let mut table = Table::new();
        table
            .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
            .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
            .set_header(vec!["Version", "Name", "Kind"]);
        for plan in migrator.plans() {
            table.add_row(vec![
                Cell::new(if let Some(new_version) = plan.script().new_version() {
                    if plan.script().version() != new_version {
                        format!("{} -> {}", plan.script().version(), new_version)
                    } else {
                        plan.script().version().to_string()
                    }
                } else {
                    plan.script().version().to_string()
                }),
                Cell::new(plan.script().name()),
                Cell::new(plan.script().kind().to_string()).fg(match plan.script().kind() {
                    dbmigrator::RecipeKind::Baseline => comfy_table::Color::Cyan,
                    dbmigrator::RecipeKind::Upgrade => comfy_table::Color::Green,
                    dbmigrator::RecipeKind::Fixup => comfy_table::Color::Yellow,
                    dbmigrator::RecipeKind::Revert => comfy_table::Color::Red,
                }),
            ]);
        }
        if let Some(target_version) = &migrator.config().target_version {
            table.add_row(vec![
                Cell::new(target_version).fg(comfy_table::Color::Magenta),
                Cell::new(""),
                Cell::new("target").fg(comfy_table::Color::Magenta),
            ]);
        }
        println!("Pending migrations:\n{table}");
    }
}

fn show_log(logs: &Vec<Changelog>, null_as_pending: bool) -> Result<(), CliError> {
    let mut table = Table::new();
    table
        .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
        .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
        .set_header(vec![
            "#",
            "Version",
            "Name",
            "Checksum",
            "Applied at",
            "Duration",
        ]);
    if logs.is_empty() {
        table.add_row(vec![
            Cell::new(""),
            Cell::new(""),
            Cell::new("Log is empty.").fg(comfy_table::Color::Cyan),
        ]);
    } else {
        let format = time::format_description::parse(
            "[year]-[month]-[day] [weekday repr:short] [hour]:[minute]:[second]",
        )?;
        for log in logs {
            table.add_row(vec![
                Cell::new(log.log_id()).set_alignment(CellAlignment::Right),
                Cell::new(log.version()).fg(if log.checksum().is_none() {
                    comfy_table::Color::Red
                } else if log.is_baseline() {
                    comfy_table::Color::Cyan
                } else if log.is_fix() {
                    comfy_table::Color::Yellow
                } else if log.is_upgrade() {
                    comfy_table::Color::Green
                } else {
                    comfy_table::Color::Grey
                }),
                match log.name() {
                    Some(name) => Cell::new(name),
                    None => Cell::new("-"),
                },
                match log.checksum32() {
                    Some(checksum) => Cell::new(checksum),
                    None => Cell::new("revert").fg(comfy_table::Color::Red),
                },
                match log.finish_ts() {
                    Some(ts) => Cell::new(ts.format(&format)?),
                    None => Cell::new(if null_as_pending {
                        "pending"
                    } else {
                        "unknown"
                    })
                    .fg(comfy_table::Color::Yellow),
                },
                match (log.start_ts(), log.finish_ts()) {
                    (Some(start_ts), Some(finish_ts)) => {
                        let dur = (finish_ts - start_ts).whole_seconds().seconds();
                        let mut cell = Cell::new(format!("{}", dur));
                        if dur >= 3600.seconds() {
                            cell = cell.fg(comfy_table::Color::Red);
                        } else if dur >= 60.seconds() {
                            cell = cell.fg(comfy_table::Color::Yellow);
                        };
                        cell
                    }
                    (_, _) => Cell::new(""),
                },
            ]);
        }
    }
    println!("{table}");
    Ok(())
}

async fn migrate(
    migrator: &mut Migrator,
    driver: &mut AsyncDriver,
    start: &Instant,
) -> Result<(), CliError> {
    let len = migrator.plans().len();

    let green_bold = Style::new().green().bold();
    let red_bold = Style::new().red().bold();
    if 0 < len {
        let pb = ProgressBar::new(len as u64);
        pb.set_style(
            ProgressStyle::with_template(
                // note that bar size is fixed unlike cargo which is dynamic
                // and also the truncation in cargo uses trailers (`...`)
                if Term::stdout().size().1 > 80 {
                    "{prefix:>12.cyan.bold} [{bar:57}] {pos}/{len} {wide_msg}"
                } else {
                    "{prefix:>12.cyan.bold} [{bar:57}] {pos}/{len}"
                },
            )
            .unwrap()
            .progress_chars("=> "),
        );
        pb.set_prefix("Database migration");

        let mut result = Ok(());
        for plan in migrator.plans() {
            pb.set_message(format!("Applying {}...", plan.script(),));
            result = migrator.apply_plan(driver.get_async_client(), plan).await;

            let err_text;
            let line = format!(
                "{:>12} {}",
                match &result {
                    Ok(_) => green_bold.apply_to("Applied"),
                    Err(e) => {
                        err_text = format!("Failed - {}", e.to_string());
                        red_bold.apply_to(err_text.as_str())
                    }
                },
                plan.script(),
            );
            pb.println(line);

            if result.is_err() {
                break;
            }
            pb.inc(1);
        }
        pb.finish_and_clear();

        if result.is_ok() {
            // migration is finished
            println!(
                "{:>12} Database migrated in {}",
                green_bold.apply_to("Finished"),
                HumanDuration(start.elapsed())
            );
        }

        result.map_err(|e| e.into())
    } else {
        // migration is finished
        println!(
            "{:>12} No pending migrations.",
            green_bold.apply_to("Finished"),
        );
        Ok(())
    }
}

fn migrator_command(cli: &Cli) -> Result<(), CliError> {
    let start = Instant::now();
    let mut config = Config::default();
    config.auto_initialize = cli.auto_initialize;
    config.log_table_name = Some(cli.changelog_table_name.clone());
    config.suggested_baseline_version = cli.suggested_baseline_version.clone();
    config.target_version = cli.target_version.clone();
    config.allow_fixes = cli.allow_fixes;
    config.allow_out_of_order = cli.allow_out_of_order;
    config.apply_by = Some(format!(
        "{} {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    ));

    let sql_files = dbmigrator::find_sql_files(cli.migrations.as_path())?;

    let mut migration_scripts = Vec::new();
    dbmigrator::load_sql_recipes(
        &mut migration_scripts,
        sql_files,
        SIMPLE_FILENAME_PATTERN,
        Some(simple_kind_detector),
    )?;

    let mut migrator = Migrator::new(config, simple_compare);

    migrator.set_recipes(migration_scripts)?;

    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async move {
        let mut driver = AsyncDriver::connect(cli.db_url.clone().unwrap().as_str()).await?;
        match cli.command {
            Some(Command::ShowConfig) => {
                show_config(&migrator);
                Ok(())
            }
            Some(Command::ShowPlan)
            | Some(Command::ShowChangelog(_))
            | Some(Command::Status(_))
            | Some(Command::Migrate(_)) => {
                migrator.read_changelog(driver.get_async_client()).await?;
                migrator.make_plan()?;
                match cli.command {
                    Some(Command::ShowPlan) => {
                        println!("Loaded migration scripts: {}", migrator.recipes().len());
                        show_plan(&migrator);

                        migrator.check_updated_log()?;
                        Ok(())
                    }
                    Some(Command::ShowChangelog(args)) => {
                        let logs = if args.with_pending {
                            migrator.updated_logs()
                        } else if args.consolidated {
                            migrator.consolidated_logs()
                        } else {
                            migrator.raw_logs()
                        };
                        show_log(logs, args.with_pending)?;
                        Ok(())
                    }
                    Some(Command::Migrate(_args)) => {
                        migrator.check_updated_log()?;
                        migrate(&mut migrator, &mut driver, &start).await?;
                        Ok(())
                    }
                    Some(Command::Status(_args)) => {
                        migrator.check_updated_log()?;
                        if migrator.plans().is_empty() {
                            println!("up-to-date");
                        } else {
                            println!("pending-migrations");
                            std::process::exit(10);
                        }
                        Ok(())
                    }
                    _ => Err(CliError::NotImplemented),
                }
            }
            _ => Err(CliError::NotImplemented),
        }
    })
}
