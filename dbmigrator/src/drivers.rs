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
use ::tokio_postgres::{Client, Connection, NoTls, Socket};

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
    client: Box<dyn AsyncClient>,
}

impl AsyncDriver {
    pub async fn connect(db_url: &str) -> Result<Self, MigratorError> {
        #[cfg(feature = "tokio-postgres")]
        {
            Self::connect_config(&db_url.parse::<::tokio_postgres::Config>()?).await
        }
        #[cfg(not(feature = "tokio-postgres"))]
        {
            let _ = db_url;
            panic!("tried to migrate from config for a postgresql database, but feature postgres not enabled!");
        }
    }

    /// Connects with an already parsed PostgreSQL configuration.
    #[cfg(feature = "tokio-postgres")]
    pub async fn connect_config(config: &::tokio_postgres::Config) -> Result<Self, MigratorError> {
        let (pgclient, connection) = config.connect(NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("connection error: {}", e);
            }
        });
        Ok(Self {
            client: Box::new(pgclient),
        })
    }

    pub fn get_async_client(&mut self) -> &mut dyn AsyncClient {
        self.client.as_mut()
    }
}
