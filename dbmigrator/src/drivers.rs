#[cfg(feature = "tokio-postgres")]
mod tokio_postgres;

//#[cfg(feature = "mysql_async")]
//pub mod mysql_async;

//#[cfg(feature = "tiberius")]
//pub mod tiberius;

use crate::changelog::Changelog;
use crate::migrator::MigrationPlan;
use crate::migrator::MigratorError;

#[cfg(feature = "tokio-postgres")]
use ::tokio_postgres::tls::NoTlsStream;
#[cfg(feature = "tokio-postgres")]
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
    client: Box<dyn AsyncClient>,
}

impl AsyncDriver {
    pub async fn connect(db_url: &str) -> Result<Self, MigratorError> {
        let client: Box<dyn AsyncClient>;
        #[cfg(feature = "tokio-postgres")]
        {
            let (pgclient, connection) = pg_connect(db_url, NoTls).await?;
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    eprintln!("connection error: {}", e);
                }
            });
            client = Box::new(pgclient);
        }
        #[cfg(not(feature = "tokio-postgres"))]
        {
            panic!("tried to migrate from config for a postgresql database, but feature postgres not enabled!");
        }
        Ok(Self {
            db_url: db_url.to_string(),
            client,
        })
    }

    pub fn get_async_client(&mut self) -> &mut dyn AsyncClient {
        self.client.as_mut()
    }
}
