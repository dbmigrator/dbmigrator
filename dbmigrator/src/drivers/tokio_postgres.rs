use super::AsyncClient;
use crate::changelog::Changelog;
use crate::migrator::MigrationPlan;
use crate::migrator::MigratorError;
use async_trait::async_trait;
use time::OffsetDateTime;
use tokio_postgres::error::SqlState;
use tokio_postgres::Client;

// TODO: Remove cast and fix error in fn log_count.
pub(crate) const LAST_LOG_ID_QUERY: &str =
    "SELECT max(log_id) AS last_log_id FROM %LOG_TABLE_NAME%;";

pub(crate) const CREATE_TABLE_QUERY: &str = "CREATE TABLE IF NOT EXISTS %LOG_TABLE_NAME%(
    log_id integer NOT NULL PRIMARY KEY,
    version text NOT NULL,
    name text,
    kind text NOT NULL,
    checksum text,
    apply_by text,
    start_ts timestamptz,
    finish_ts timestamptz,
    revert_ts timestamptz
);";

pub(crate) const GET_LOG_QUERY: &str = "SELECT log_id, version, name, kind, checksum, apply_by, start_ts, finish_ts, revert_ts FROM %LOG_TABLE_NAME% ORDER BY log_id ASC;";

#[async_trait]
impl AsyncClient for Client {
    async fn last_log_id(&mut self, log_table_name: &str) -> Result<i32, MigratorError> {
        let result = self
            .query_opt(
                &LAST_LOG_ID_QUERY.replace("%LOG_TABLE_NAME%", log_table_name),
                &[],
            )
            .await;

        match result {
            Ok(Some(row)) => Ok(row.get::<usize, Option<i32>>(0).unwrap_or(0)),
            Ok(None) => Ok(-1),
            Err(e) => {
                if let Some(db_error) = e.as_db_error() {
                    if db_error.code().eq(&SqlState::UNDEFINED_TABLE) {
                        Err(MigratorError::NoLogTable())
                    } else {
                        Err(MigratorError::PgError(e))
                    }
                } else {
                    Err(MigratorError::PgError(e))
                }
            }
        }
    }

    async fn get_changelog(
        &mut self,
        log_table_name: &str,
    ) -> Result<Vec<Changelog>, MigratorError> {
        let transaction = self.transaction().await?;
        transaction
            .execute(
                &CREATE_TABLE_QUERY.replace("%LOG_TABLE_NAME%", log_table_name),
                &[],
            )
            .await?;

        let rows = transaction
            .query(
                &GET_LOG_QUERY.replace("%LOG_TABLE_NAME%", log_table_name),
                &[],
            )
            .await?;
        let mut log = Vec::new();
        for row in rows.into_iter() {
            let log_id = row.get(0);
            let version = row.get(1);
            let name = row.get(2);
            let kind = row.get(3);
            let checksum = row.get(4);
            let apply_by = row.get(5);
            let start_ts = row.get(6);
            let finish_ts = row.get(7);
            let revert_ts = row.get(8);

            log.push(Changelog::new(
                log_id, version, name, kind, checksum, apply_by, start_ts, finish_ts, revert_ts,
            ));
        }
        transaction.commit().await?;
        Ok(log)
    }

    async fn apply_plan(
        &mut self,
        log_table_name: &str,
        plan: &MigrationPlan,
    ) -> Result<(), MigratorError> {
        let transaction = self.transaction().await?;
        let rows = transaction.query("SELECT clock_timestamp();", &[]).await?;
        let start_ts: Option<OffsetDateTime> = match rows.iter().next() {
            Some(row) => row.get(0),
            None => None,
        };
        transaction.batch_execute(plan.sql()).await?;
        if let Some(log_to_revert) = plan.log_id_to_revert() {
            transaction
                .execute(
                    &format!(
                        "UPDATE {} SET revert_ts = $2 WHERE log_id = $1;",
                        log_table_name
                    ),
                    &[&log_to_revert, &start_ts],
                )
                .await?;
        }
        #[cfg(debug_assertions)]
        {
            transaction
                .batch_execute("SELECT pg_sleep(random()*2);")
                .await?;
        }
        let rows = transaction.query("SELECT clock_timestamp();", &[]).await?;
        let finish_ts: Option<OffsetDateTime> = match rows.iter().next() {
            Some(row) => row.get(0),
            None => None,
        };
        if let Some(log) = plan.revert_log() {
            transaction.execute(
                &format!(
                    "INSERT INTO {} (log_id, version, name, kind, checksum, apply_by, start_ts, finish_ts) VALUES ($1, $2, $3, $4, $5, $6, $7, $8);",
                    log_table_name
                ),
                &[
                    &log.log_id(),
                    &log.version(),
                    &log.name(),
                    &log.kind_str(),
                    &log.checksum(),
                    &log.apply_by(),
                    &start_ts,
                    &finish_ts,
                ],
            ).await?;
        }
        if let Some(log) = plan.apply_log() {
            transaction.execute(
                &format!(
                    "INSERT INTO {} (log_id, version, name, kind, checksum, apply_by, start_ts, finish_ts) VALUES ($1, $2, $3, $4, $5, $6, $7, $8);",
                    log_table_name
                ),
                &[
                    &log.log_id(),
                    &log.version(),
                    &log.name(),
                    &log.kind_str(),
                    &log.checksum(),
                    &log.apply_by(),
                    &start_ts,
                    &finish_ts,
                ],
            ).await?;
        }
        transaction.commit().await?;
        Ok(())
    }
}
