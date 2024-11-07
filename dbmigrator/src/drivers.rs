#[cfg(feature = "tokio-postgres")]
mod tokio_postgres;

//#[cfg(feature = "mysql_async")]
//pub mod mysql_async;

//#[cfg(feature = "tiberius")]
//pub mod tiberius;

use crate::changelog::Changelog;
use crate::migrator::MigrationPlan;
use crate::migrator::MigratorError;
use ::tokio_postgres::tls::NoTlsStream;
use ::tokio_postgres::{connect as pg_connect, Client, Connection, NoTls, Socket};
use async_trait::async_trait;

#[async_trait]
pub trait AsyncClient {
    async fn last_log_id(&mut self, log_table_name: &str) -> Result<i32, MigratorError>;
    async fn get_changelog(
        &mut self,
        log_table_name: &str,
    ) -> Result<Vec<Changelog>, MigratorError>;
    async fn apply_plan(
        &mut self,
        log_table_name: &str,
        plan: &MigrationPlan,
    ) -> Result<(), MigratorError>;
}

pub struct AsyncDriver {
    db_url: String,
    client: Client,
}

impl AsyncDriver {
    pub async fn connect(db_url: &str) -> Result<Self, MigratorError> {
        let (client, connection) = pg_connect(db_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("connection error: {}", e);
            }
        });
        Ok(Self {
            db_url: db_url.to_string(),
            client,
        })
    }

    pub fn get_async_client(&mut self) -> &mut impl AsyncClient {
        &mut self.client
    }
}
