Powerful SQL migration toolkit for Rust.

[![MIT licensed][mit-badge]][mit-url]

[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: LICENSE-MIT

## Usage

- Add *dbmigrator* to your Cargo.toml dependencies with the selected driver as feature eg: `dbmigrator = { version = "0.8", features = ["tokio_postgres"]}`
- Migrations can be defined in .sql files.

## Intoduction

DBMigrator is a database schema change management solution that enables you 
to revise and release database changes faster and safer from development to production.

### Recipes

**Recipe** is the basic unit of change in DBMigrator. It is a simple SQL script executed in a transaction. 

We distinguish four kinds of recipes:
- **baseline** - A consolidated SQL script independently creating a specific version of the database.
- **upgrade** - The smallest unit of change, migrating the DB schema one version forward. 
- **revert** - A special fix script to roll back erroneous upgrade recipes that need to be and can be reverted.
- **fixup** - Also a fix script that can attempt to repair an erroneously issued upgrade recipe. 
  Sometimes it is not possible to reverse the action of an erroneously issued migration.
  This kind of recipe can, however, introduce some corrective actions.

We do not foresee creating pairs of *do* and *undo* scripts because 
it is usually not possible to roll back applied migrations.
*Undo* scripts are typically not eagerly maintained by developers, are usually untested, and are just a source of additional problems.

Each recipe is identified by a *version* and a *name*.

We must choose a version sorting algorithm because recipes need to be arranged in an unambiguous order.
Two version comparators are available: 
- **Simple compare** assumes that the version should be fixed length to avoid problems with comparing numeric parts.
- **Version compare** allows naming files according to semver, 
  but most file tools will not be able to present them in the correct order. 
  
If not necessary, it is better to use `simple_compare` and try to use versions with a uniform number of characters.

Recipes have the following metadata:

| Field               | Description                                                      | Example                                                 |
|---------------------|------------------------------------------------------------------|---------------------------------------------------------|
| **version**         | Unique and sortable.                                             | `20241106-1231`, `000001`, `1.2.0-001`                  |
| **name**            | Name of the recipe. Recommended.                                 | `create_table_customer`, `baseline`, `up`, `revert`     |
| **checksum**        | SHA2-256 of SQL file                                             | 128 chars of lowercase hex.                             |
| **kind**            | Type of recipe. Detected from name usually.                      | `baseline`, `upgrade`, `revert`, `fixup`                |
| **old_checksum**    | Checksum of recipe to fix. Required for `revert` and `fixup`.    | You can use full checksum or only first 8 chars of hex. |
| **maximum_version** | Maximum version of current DB, when fix can be applied.          |                                                         |
| **new_version**     | For `fixup`. The old changelog entry will be replaced with this. |                                                         |
| **new_name**        | For `fixup`. The old changelog entry will be replaced with this. |                                                         |
| **new_checksum**    | For `fixup`. The old changelog entry will be replaced with this. | You have to put all 128 chars.                          |

All metadata can be stored in the SQL file as first comments:

```sql
-- old_checksum: ab3c3ade
-- maximum_version: 20241106-2359

DROP TABLE IF EXISTS customer;
```

### Changelog

Changes are stored in a changelog table. It is a simple table with the following columns:

| Name       | Type                  | Description                                               |
|------------|-----------------------|-----------------------------------------------------------|
| log_id     | integer NOT NULL      | Unique serial and primary key                             |
| version    | varchar(255) NOT NULL | Version                                                   |
| name       | varchar(255)          | Name of the recipe                                        |
| kind       | varchar(10) NOT NULL  | Type of recipe (`baseline`, `upgrade`, `revert`, `fixup`) |
| checksum   | varchar(255)          | SHA2-256 of recipe (NULL for revert)                      |
| applied_by | varchar(255)          | Application/user/etc which/who applied the recipe         |
| start_ts   | timestamptz           | When the recipe applaying was started                     |
| finish_ts  | timestamptz           | When the recipe applaying was finished                    |
| revert_ts  | timestamptz           | When the recipe was reverted                              |

`log_id` is plain integer, not database serial. DBMigrator automatically increments it from 1.

DBMigrator determines the effective migration state by reviewing subsequent changelog entries 
in the `dbmigrator_log` table. For each version, the last row is considered the current one.
Rows with a checksum equal to NULL remove (revert) the effective state for a given version.

Historical changelog row is never deleted and modified. Only `revert_ts` is updated when the recipe 
is reverted or amended. `revert_ts` is only informative and does not affect the effective state.

First row in the `dbmigrator_log` table is always the baseline. It is created automatically 
when the database is initialized. 

The basic strategy assumes that we create an empty baseline file for the first version 
(e.g., `0.0.0_baseline.sql`, `000000_baseline.sql`, etc.).
All following database changes are further `upgrade` migrations.
When initializing a new database, we execute all recipes sequentially.

This can be optimized by periodically issuing a baseline recipe for release versions 
with a consolidated SQL script.
When initializing a new database, the latest baseline version will be used.

## License

This project is licensed under the [MIT license](LICENSE-MIT) and [Apache license](LICENSE-APACHE-2.0).

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in dbmigrator by you, shall be licensed as MIT, without any additional
terms or conditions.

### Inspiration and ideas

- Katharina Fey (Refinery) - kookie@spacekookie.de
- João Oliveira (Refinery) - hello@jxs.pt