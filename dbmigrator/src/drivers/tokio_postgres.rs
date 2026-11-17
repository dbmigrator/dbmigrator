use super::AsyncClient;
use crate::changelog::Changelog;
use crate::migrator::MigrationPlan;
use crate::migrator::MigratorError;
use async_trait::async_trait;
use time::OffsetDateTime;
use tokio_postgres::error::{ErrorPosition, SqlState};
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
        apply_plan_in_transaction(self, log_table_name, plan)
            .await
            .map_err(|e| match e {
                MigratorError::PgError(source) => MigratorError::ScriptFailed {
                    script: plan.script().clone(),
                    position: None,
                    source,
                },
                e => e,
            })
    }
}

async fn apply_plan_in_transaction(
    client: &mut Client,
    log_table_name: &str,
    plan: &MigrationPlan,
) -> Result<(), MigratorError> {
    let transaction = client.transaction().await?;
    let rows = transaction
        .query(
            "SELECT clock_timestamp(), current_setting('server_encoding');",
            &[],
        )
        .await?;
    let (start_ts, server_encoding): (Option<OffsetDateTime>, Option<String>) =
        match rows.iter().next() {
            Some(row) => (row.get(0), row.get(1)),
            None => (None, None),
        };
    transaction
        .batch_execute(plan.sql())
        .await
        .map_err(|source| {
            let position = match source.as_db_error().and_then(|db| db.position()) {
                Some(ErrorPosition::Original(pos)) => Some(line_col(
                    plan.sql(),
                    *pos,
                    server_encoding.as_deref() == Some("SQL_ASCII"),
                )),
                _ => None,
            };
            MigratorError::ScriptFailed {
                script: plan.script().clone(),
                position,
                source,
            }
        })?;
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

/// Converts a 1-based error position in `sql`, as reported by PostgreSQL, to a 1-based line and
/// character column. The server counts characters, except in `SQL_ASCII` databases, where it
/// counts bytes.
fn line_col(sql: &str, pos: u32, pos_in_bytes: bool) -> (usize, usize) {
    let offset = (pos as usize).saturating_sub(1);
    let chars = if pos_in_bytes {
        sql.char_indices().take_while(|(i, _)| *i < offset).count()
    } else {
        offset
    };
    let mut line = 1;
    let mut column = 1;
    for c in sql.chars().take(chars) {
        if c == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::line_col;

    #[test]
    fn line_col_first_line() {
        assert_eq!(line_col("SELECT foo;", 1, false), (1, 1));
        assert_eq!(line_col("SELECT foo;", 8, false), (1, 8));
    }

    #[test]
    fn line_col_after_newlines() {
        assert_eq!(line_col("SELECT 1;\nSELECT foo;", 18, false), (2, 8));
        assert_eq!(line_col("SELECT 1;\r\nSELECT foo;", 19, false), (2, 8));
    }

    #[test]
    fn line_col_character_positions() {
        assert_eq!(line_col("-- żółć\nSELECT foo;", 16, false), (2, 8));
    }

    #[test]
    fn line_col_byte_positions() {
        // "żółć" is 4 characters, 8 bytes.
        assert_eq!(line_col("-- żółć\nSELECT foo;", 20, true), (2, 8));
        assert_eq!(line_col("-- żółć\nSELECT foo;", 4, true), (1, 4));
        // An offset inside a multi-byte character points at the next character.
        assert_eq!(line_col("żółć", 2, true), (1, 2));
    }

    #[test]
    fn line_col_past_end() {
        assert_eq!(line_col("SELECT", 100, false), (1, 7));
        assert_eq!(line_col("SELECT", 100, true), (1, 7));
    }
}
