/*!
Powerful SQL migration toolkit for Rust.

TODO - Work in progress... DO NOT USE IN PRODUCTION - COME BACK IN 2025 JANUARY.

`dbmigrator` makes running migrations for different databases as easy as possible.
It works by running your migrations on a provided database connection, either by embedding them on your Rust code, or via `dbmigrator_cli`.\
Currently, [`Postgres`](https://crates.io/crates/postgres), are supported.\
Planned [`Mysql`](https://crates.io/crates/mysql).\

`dbmigrator` works with .sql file migrations.

## Usage

- Migrations can be defined in .sql files.
- Migrations must be named in the format `{1}_{2}.sql` where `{1}` represents the migration version, `{2}` migration kind (upgrade, baseline, revert or fixup) and name.
- Migrations can be run either by embedding them on your Rust code with [`embed_migrations!`] macro, or via `dbmigrator_cli`.

[`embed_migrations!`]: macro.embed_migrations.html

### Example
```rust,ignore
use rusqlite::Connection;

mod embedded {
    use dbmigrator::embed_migrations;
    embed_migrations!("./tests/sql_migrations");
}

let mut conn = Connection::open_in_memory().unwrap();
let mut migrator = embedded::migrations::migrator();
migrator.read_changelog(&mut conn).await.unwrap();
migrator.make_plan();
migrator.check_updated_log().unwrap();
for plan in migrator.plans() {
    migrator.apply_plan(&mut conn, plan).await.unwrap();
}
```

for more examples refer to the [examples](https://github.com/dbmigrator/dbmigrator/tree/master/examples)
*/

mod changelog;
mod drivers;
mod migrator;

use dbmigrator_core::recipe;

pub use dbmigrator_macros::embed_migrations;

pub use changelog::Changelog;
pub use drivers::{AsyncClient, AsyncDriver};
pub use migrator::Config;
pub use migrator::MigrationPlan;
pub use migrator::Migrator;
pub use migrator::MigratorError;
pub use recipe::find_sql_files;
pub use recipe::load_sql_recipes;
pub use recipe::RecipeError;
pub use recipe::RecipeKind;
pub use recipe::RecipeScript;
pub use recipe::SIMPLE_FILENAME_PATTERN;
pub use recipe::{simple_compare, simple_kind_detector, version_compare};

#[doc(hidden)]
pub use dbmigrator_core as __core;
