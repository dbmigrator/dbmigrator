/*!
Powerful SQL migration toolkit for Rust.

`dbmigrator` makes running migrations for different databases as easy as possible.
It works by running your migrations on a provided database connection, either by embedding them on your Rust code, or via `dbmigrator_cli`.\
Currently, [`Postgres`](https://crates.io/crates/postgres), [`Rusqlite`](https://crates.io/crates/rusqlite), and [`Mysql`](https://crates.io/crates/mysql) are supported.\

`dbmigrator` works with .sql file migrations.

## Usage

- Migrations can be defined in .sql files.
- Migrations must be named in the format `{1}_{2}_{3}.rs ` where `{1}` represents the migration version, `{2}` migration type (upgrade, baseline or revert) and `{3}` the optional name.
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
embedded::migrations::runner().run(&mut conn).unwrap();
```

for more examples refer to the [examples](https://github.com/dbmigrator/dbmigrator/tree/master/examples)
*/

mod changelog;
mod drivers;
mod migrator;
mod recipe;

pub use changelog::Changelog;
pub use drivers::{AsyncClient, AsyncDriver};
pub use migrator::Config;
pub use migrator::Migrator;
pub use migrator::MigratorError;
pub use recipe::find_sql_files;
pub use recipe::load_sql_recipes;
pub use recipe::RecipeError;
pub use recipe::RecipeKind;
pub use recipe::RecipeScript;
pub use recipe::SIMPLE_FILENAME_PATTERN;
pub use recipe::{simple_compare, simple_kind_detector, version_compare};
