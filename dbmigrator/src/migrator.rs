use crate::changelog::Changelog;
use crate::drivers::AsyncClient;
use crate::recipe::{order_recipes, RecipeKind, RecipeScript};
use crate::RecipeError;
use std::cmp::Ordering;
use thiserror::Error;
#[cfg(feature = "tokio-postgres")]
use tokio_postgres::error::Error as PgError;

/// An Error occurred during a migration cycle
#[derive(Debug, Error)]
pub enum MigratorError {
    #[error(transparent)]
    RecipeError(RecipeError),

    #[error("no baseline migration available")]
    NoBaseline(),

    #[error("unknown baseline for version {0}")]
    UnknownBaseline(String),

    #[error("unknown target version {version} (try {})", .available.as_deref().unwrap_or("-"))]
    UnknownTarget {
        version: String,
        available: Option<String>,
    },

    #[error("no dbmigrator_log table available")]
    NoLogTable(),

    #[error("unknown migration in database `{log}`")]
    UnknownMigration { log: Changelog },

    #[error("missing migration in database `{script}`")]
    MissingMigration { script: RecipeScript },

    #[error("conflicted migration - db: `{log}`, script: `{script}`")]
    ConflictedMigration {
        log: Changelog,
        script: RecipeScript,
    },

    #[error(transparent)]
    PgError(PgError),
}

impl From<RecipeError> for MigratorError {
    fn from(err: RecipeError) -> MigratorError {
        MigratorError::RecipeError(err)
    }
}

#[cfg(feature = "tokio-postgres")]
impl From<PgError> for MigratorError {
    fn from(err: PgError) -> MigratorError {
        MigratorError::PgError(err)
    }
}

#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Config {
    /// Allow create dbmigrator log table if not exists.
    pub auto_initialize: bool,

    /// Table name for dbmigrator_log table (should include DB schema).
    pub log_table_name: Option<String>,

    /// Baseline for initialization (if not defined use last available baseline).
    pub suggested_baseline_version: Option<String>,

    /// Limit migration to specified version (if not defined apply all).
    pub target_version: Option<String>,

    /// Optional description of the application that applies migrations.
    pub apply_by: Option<String>,

    /// Allow to apply revert and fixup migrations
    pub allow_fixes: bool,

    /// Allow to out of order migrations
    pub allow_out_of_order: bool,
}

impl Config {
    pub fn effective_log_table_name(&self) -> &str {
        self.log_table_name.as_deref().unwrap_or("dbmigrator_log")
    }
}

fn update_agg_log<'a>(
    agg_log: &mut Vec<Changelog>,
    version_comparator: fn(&str, &str) -> std::cmp::Ordering,
    log: &Changelog,
) {
    match (
        agg_log.binary_search_by(|a| (version_comparator)(&a.version(), log.version())),
        log.checksum().is_some(),
    ) {
        (Err(index), true) => {
            agg_log.insert(index, log.clone());
        }
        (Err(_), false) => (),
        (Ok(index), true) => {
            agg_log[index] = log.clone();
        }
        (Ok(index), false) => {
            agg_log.remove(index);
        }
    }
}

fn find_agg_log<'a>(
    agg_log: &'a Vec<Changelog>,
    version_comparator: fn(&str, &str) -> std::cmp::Ordering,
    version: &str,
) -> Option<&'a Changelog> {
    match agg_log.binary_search_by(|a| (version_comparator)(&a.version(), version)) {
        Ok(index) => Some(&agg_log[index]),
        Err(_) => None,
    }
}

/*
1. Sprawdzamy wersję ostatniej migracji (`current_version`) w bazie.
2. Jeśli brak tabeli dziennika to:
    - gdy `auto_initialize` to tworzymy tabelę dziennika
    - gdy brak `auto_initialize` to zwracamy błąd
3. Weryfikujemy integralność skryptów migracji:
    - sprawdzamy czy skrypty fixup nie dotyczą istniejących skryptów migracji (niedozwolone),
    - sprawdzamy czy skrypty fixup z akcją `change_checksum` kierują do istniejących skryptów,
    - sprawdzamy czy nie ma zdublowanych skryptów upgradu (tylko jeden na wersję),
    - sprawdzamy czy nie ma zdublowanych skryptów baseline (tylko jeden na wersję),
4. Wczytujemy dziennik migracji z bazy.
6. Gdy `allow_pending_fixup = true` to:
    - poczynając od pierwszej do ostatniej migracji z dziennika sprawdzamy czy istnieją dla niej fixupy
    - jak fixup może być zastosowany (spełniony warunek maximum_version) to dodajemy do planu
    - jeśli zastosowaliśmy jakikolwiek fixup z sukcesem to ponawiamy sprawdzenie całej listy od nowa
7. Z dziennika ustalamy `baseline_version`.
8. Gdy brak `baseline_version` to:
    - do planu wstawiamy migrację baseline (`suggested_baseline_version` albo najstarszą z dostępnych)
    - jak brak w/w to błąd
    - ustalamy `baseline_version` na w/w migrację
9. Mając `baseline_version` i ewentualnie `target_version` filtrujemy listę skryptów migracji rodzaju `upgrade` do weryfikacji.
10. Następnie wryfikujemy listę dziennika, czy wszystkie wpisy są zgodne z listą skryptów migracji.
    - Gdy `allow_out_of_order` to dana migracja musi istnieć. Gdy nie `allow_out_of_order`, to to dana migracja musi być pierwsza na liście.
    - Zgodne wpisy usuwamy z listy skryptów migracji.
11. Wpisy, które pozostały trafiają do planu migracji.
12. Wykonujemy plan migracji.
 */
pub struct Migrator {
    config: Config,
    version_comparator: fn(&str, &str) -> std::cmp::Ordering,
    recipes: Vec<RecipeScript>,
    last_log_id: i32,
    next_log_id: i32,
    raw_logs: Vec<Changelog>,
    consolidated_logs: Vec<Changelog>,
    updated_logs: Vec<Changelog>,
    baseline_version: Option<String>,
    plans: Vec<MigrationPlan>,
}

impl Migrator {
    pub fn new(config: Config, version_comparator: fn(&str, &str) -> std::cmp::Ordering) -> Self {
        Migrator {
            config,
            version_comparator,
            recipes: Vec::new(),
            last_log_id: 0,
            next_log_id: 1,
            raw_logs: Vec::new(),
            consolidated_logs: Vec::new(),
            updated_logs: Vec::new(),
            baseline_version: None,
            plans: Vec::new(),
        }
    }

    fn finder(&self) -> impl Fn(&RecipeScript, &str, RecipeKind) -> std::cmp::Ordering + use<'_> {
        |item: &RecipeScript, version: &str, kind: RecipeKind| {
            (self.version_comparator)(item.version(), version).then_with(|| item.kind().cmp(&kind))
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn recipes(&self) -> &Vec<RecipeScript> {
        &self.recipes
    }

    pub fn raw_logs(&self) -> &Vec<Changelog> {
        &self.raw_logs
    }

    pub fn consolidated_logs(&self) -> &Vec<Changelog> {
        &self.consolidated_logs
    }

    pub fn updated_logs(&self) -> &Vec<Changelog> {
        &self.updated_logs
    }

    pub fn plans(&self) -> &Vec<MigrationPlan> {
        &self.plans
    }

    pub fn set_recipes(&mut self, mut recipes: Vec<RecipeScript>) -> Result<(), MigratorError> {
        order_recipes(&mut recipes, self.version_comparator)?;
        self.recipes = recipes;
        Ok(())
    }

    /// Read changelog from the database and consolidate it to an ordered and effective list.
    pub async fn read_changelog(
        &mut self,
        client: &mut impl AsyncClient,
    ) -> Result<(), MigratorError> {
        let last_log_id = client
            .last_log_id(self.config.effective_log_table_name())
            .await;
        match last_log_id {
            Ok(last_log_id) => {
                self.last_log_id = last_log_id;
            }
            Err(MigratorError::NoLogTable()) => {
                if !self.config.auto_initialize {
                    return Err(MigratorError::NoLogTable());
                }
                self.last_log_id = 0;
            }
            Err(e) => return Err(e),
        }
        self.next_log_id = self.last_log_id + 1;

        self.raw_logs = client
            .get_changelog(self.config.effective_log_table_name())
            .await?;
        self.consolidated_logs.clear();
        for log in self.raw_logs.iter() {
            update_agg_log(&mut self.consolidated_logs, self.version_comparator, log);
        }
        self.updated_logs = self.consolidated_logs.clone();

        self.plans.clear();

        Ok(())
    }

    fn recipes_for_version(&self, version: &str) -> &[RecipeScript] {
        match self
            .recipes
            .binary_search_by(|a| (self.version_comparator)(a.version(), version))
        {
            Ok(first) => {
                if let Some(last) = self.recipes[first..].iter().position(|a| {
                    (self.version_comparator)(a.version(), version) == Ordering::Greater
                }) {
                    &self.recipes[first..first + last + 1]
                } else {
                    &self.recipes[first..]
                }
            }
            Err(first) => &self.recipes[first..first],
        }
    }

    fn match_fix_recipe(
        &self,
        log_version: &str,
        log_checksum: &str,
        recipe: &RecipeScript,
        current_version: &str,
    ) -> bool {
        if let (Some(old_checksum), Some(maximum_version)) =
            (recipe.old_checksum(), recipe.maximum_version())
        {
            log_version == recipe.version()
                && log_checksum == old_checksum
                && matches!(
                    (self.version_comparator)(current_version, maximum_version),
                    std::cmp::Ordering::Less | std::cmp::Ordering::Equal
                )
        } else {
            false
        }
    }

    fn baseline_recipe(&self) -> Result<RecipeScript, MigratorError> {
        match self.config.suggested_baseline_version.as_ref() {
            Some(suggested_baseline_version) => {
                match self.recipes.binary_search_by(|a| {
                    (self.finder())(a, suggested_baseline_version, RecipeKind::Baseline)
                }) {
                    Ok(index) => Ok(self.recipes[index].clone()),
                    Err(_) => Err(MigratorError::UnknownBaseline(
                        suggested_baseline_version.to_string(),
                    )),
                }
            }
            None => match self.recipes.iter().rev().find(|&s| s.is_baseline()) {
                Some(recipe) => Ok(recipe.clone()),
                None => Err(MigratorError::NoBaseline()),
            },
        }
    }

    pub fn make_plan(&mut self) -> Result<(), MigratorError> {
        if self.config.allow_fixes {
            let mut current_version: Option<String> = None;
            let mut new_logs: Vec<Changelog> = Vec::new();
            for log in self.updated_logs.iter().rev() {
                if current_version.is_none() {
                    current_version = Some(log.version().to_string());
                }
                // TODO: Dlaczego muszę kopiować wektor poniżej?!
                let fixes = self.recipes_for_version(log.version()).to_vec();
                if let Some(fix) = fixes.iter().find(|fix| {
                    self.match_fix_recipe(
                        log.version(),
                        log.checksum().unwrap(),
                        fix,
                        &current_version.clone().unwrap(),
                    )
                }) {
                    let revert_log = Changelog::new(
                        self.next_log_id,
                        log.version().to_string(),
                        Some(fix.name().to_string()),
                        fix.kind().to_string(),
                        None,
                        self.config.apply_by.clone(),
                        None,
                        None,
                        None,
                    );
                    self.next_log_id += 1;

                    let apply_log =
                        if let Some((new_version, new_name, new_checksum)) = fix.new_target() {
                            let log = Some(Changelog::new(
                                self.next_log_id,
                                new_version.to_string(),
                                Some(new_name.to_string()),
                                fix.kind().to_string(),
                                Some(new_checksum.to_string()),
                                self.config.apply_by.clone(),
                                None,
                                None,
                                None,
                            ));
                            self.next_log_id += 1;
                            log
                        } else {
                            None
                        };
                    new_logs.push(revert_log.clone());
                    if let Some(apply_log) = apply_log.as_ref() {
                        new_logs.push(apply_log.clone());
                    }
                    self.plans.push(MigrationPlan {
                        recipe: fix.clone(),
                        log_id_to_revert: Some(log.log_id()),
                        revert_log: Some(revert_log.clone()),
                        apply_log: apply_log.clone(),
                    });
                    // We have to update current version of DB scheme. It is important for next fixups.
                    // For `Revert` we reset to None, for `Fixup` we set to new_version.
                    current_version = fix.new_version().map(|v| v.to_string());
                    break;
                }
            }
            for log in new_logs {
                update_agg_log(&mut self.updated_logs, self.version_comparator, &log);
            }
        }

        let mut last_version: String;
        if self.updated_logs.len() > 0 {
            self.baseline_version = Some(self.updated_logs[0].version().to_string());
            last_version = self.updated_logs.last().unwrap().version().to_string();
        } else {
            let baseline_recipe = self.baseline_recipe()?;
            self.baseline_version = Some(baseline_recipe.version().to_string());
            last_version = baseline_recipe.version().to_string();
            let apply_log = Changelog::new(
                self.next_log_id,
                baseline_recipe.version().to_string(),
                Some(baseline_recipe.name().to_string()),
                baseline_recipe.kind().to_string(),
                Some(baseline_recipe.checksum().to_string()),
                self.config.apply_by.clone(),
                None,
                None,
                None,
            );
            self.next_log_id += 1;
            update_agg_log(&mut self.updated_logs, self.version_comparator, &apply_log);
            self.plans.push(MigrationPlan {
                recipe: baseline_recipe,
                log_id_to_revert: None,
                revert_log: None,
                apply_log: Some(apply_log),
            });
        }
        for recipe in self
            .recipes
            .iter()
            .skip_while(|r| {
                matches!(
                    (self.version_comparator)(r.version(), &last_version),
                    Ordering::Less | Ordering::Equal
                )
            })
            .take_while(|r| match &self.config.target_version {
                Some(target_version) => matches!(
                    (self.version_comparator)(r.version(), target_version),
                    Ordering::Less | Ordering::Equal
                ),
                None => true,
            })
            .filter(|r| r.is_upgrade())
        {
            let apply_log = Changelog::new(
                self.next_log_id,
                recipe.version().to_string(),
                Some(recipe.name().to_string()),
                recipe.kind().to_string(),
                Some(recipe.checksum().to_string()),
                self.config.apply_by.clone(),
                None,
                None,
                None,
            );
            self.next_log_id += 1;
            update_agg_log(&mut self.updated_logs, self.version_comparator, &apply_log);
            self.plans.push(MigrationPlan {
                recipe: recipe.clone(),
                log_id_to_revert: None,
                revert_log: None,
                apply_log: Some(apply_log),
            });
        }
        Ok(())
    }

    pub fn check_updated_log(&self) -> Result<(), MigratorError> {
        // Check if target version is known.
        if let Some(target_version) = &self.config.target_version {
            if let Err(_) = self
                .recipes
                .binary_search_by(|a| (self.finder())(a, target_version, RecipeKind::Baseline))
            {
                if let Err(index) = self
                    .recipes
                    .binary_search_by(|a| (self.finder())(a, target_version, RecipeKind::Upgrade))
                {
                    return Err(MigratorError::UnknownTarget {
                        version: target_version.clone(),
                        available: if 1 <= index {
                            Some(self.recipes[index - 1].version().to_string())
                        } else {
                            None
                        },
                    });
                }
            }
        }

        // Check if all applied migrations in the database are known.
        for (index, log) in self.updated_logs.iter().enumerate() {
            if index > 0 {
                match self
                    .recipes
                    .binary_search_by(|a| (self.finder())(a, log.version(), RecipeKind::Upgrade))
                {
                    Ok(index) => {
                        if log.checksum().unwrap_or("") != self.recipes[index].checksum() {
                            return Err(MigratorError::ConflictedMigration {
                                log: log.clone(),
                                script: self.recipes[index].clone(),
                            });
                        }
                    }
                    Err(_) => return Err(MigratorError::UnknownMigration { log: log.clone() }),
                }
            }
        }

        // Check if all upgrade recipes are applied.
        if let Some(baseline_version) = &self.baseline_version {
            for script in self
                .recipes
                .iter()
                .skip_while(|r| {
                    matches!(
                        (self.version_comparator)(r.version(), baseline_version),
                        Ordering::Less | Ordering::Equal
                    )
                })
                .take_while(|r| match &self.config.target_version {
                    Some(target_version) => matches!(
                        (self.version_comparator)(r.version(), target_version),
                        Ordering::Less | Ordering::Equal
                    ),
                    None => true,
                })
                .filter(|r| r.is_upgrade())
            {
                match find_agg_log(
                    &self.updated_logs,
                    self.version_comparator,
                    script.version(),
                ) {
                    Some(log) => {
                        if log.checksum().unwrap_or("") != script.checksum() {
                            return Err(MigratorError::ConflictedMigration {
                                log: log.clone(),
                                script: script.clone(),
                            });
                        }
                    }
                    None => {
                        return Err(MigratorError::MissingMigration {
                            script: script.clone(),
                        })
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn apply_plan(
        &self,
        client: &mut impl AsyncClient,
        plan: &MigrationPlan,
    ) -> Result<(), MigratorError> {
        client
            .apply_plan(self.config.effective_log_table_name(), plan)
            .await?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct MigrationPlan {
    recipe: RecipeScript,
    log_id_to_revert: Option<i32>,
    revert_log: Option<Changelog>,
    apply_log: Option<Changelog>,
}

impl MigrationPlan {
    pub fn script(&self) -> &RecipeScript {
        &self.recipe
    }

    pub fn sql(&self) -> &str {
        self.recipe.sql()
    }
    pub fn log_id_to_revert(&self) -> Option<i32> {
        self.log_id_to_revert
    }
    pub fn revert_log(&self) -> Option<&Changelog> {
        self.revert_log.as_ref()
    }
    pub fn apply_log(&self) -> Option<&Changelog> {
        self.apply_log.as_ref()
    }
}
