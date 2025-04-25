Powerful SQL migration toolkit for Rust.

[![crates.io][crates-badge]][crates-url]
[![Docs.rs][docs-badge]][docs-url]
[![MIT licensed][mit-badge]][mit-url]

[crates-badge]: https://img.shields.io/crates/v/dbmigrator.svg
[crates-url]: https://crates.io/crates/dbmigrator
[docs-badge]: https://img.shields.io/docsrs/dbmigrator.svg
[docs-url]: https://docs.rs/dbmigrator/
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: LICENSE-MIT

## Usage

- Add *dbmigrator* to your Cargo.toml dependencies with the selected driver as feature eg:
  `dbmigrator = { version = "0.8", features = ["tokio_postgres"]}`
- Migrations can be defined in .sql files.

## Introduction

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
*Undo* scripts are typically not eagerly maintained by developers, are usually untested, and are just a source of
additional problems.

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

### DB users

Some DDL commands (and thus migrations) may require superuser privileges:
- adding untrusted extensions such as `postgis`, `file_fdw`, `timescaledb_toolkit`,
- adding functions in untrusted languages (e.g., PL/Python).

Since the above requirements are frequent enough, an architectural decision was made
that DBMigrator will connect to the database as a superuser.

Additionally, many bootstrapping tasks are much easier to perform as a superuser:
- creating necessary roles/groups,
- creating the database,
- changing the owner of the objects,
- changing permissions.

On the other hand, the application in normal mode should connect to the database using 
a regular role with limited privileges.

### DB ACL

Security requirements demand that different applications connect to the database 
using different users with different passwords and permissions.

Most SQL implementations separate:
- LOGIN (users with passwords for authentication) and
- ROLE/GROUP (owners of objects and recipients of ACL permissions).

DBMigrator maintains this division, even though PostgreSQL combines 
these two types of objects into a single global role list, common to all databases:

1. LOGIN roles are independent for subsequent databases (and have, among other things, different passwords)
2. the beginnings of LOGIN role names are consistent with the names of the databases they pertain to,
3. LOGIN roles are never part of the database schema (i.e., they will not appear as object owners and in ACL),
4. the schema refers only to NOLOGIN roles, which do not have passwords and are common to all instances/copies of a given database,
5. it is recommended that NOLOGIN role names also have a common prefix and it must not conflict with the potential name of a new database.

### DDL

**DDL** (Data Definition Language) is a subset of SQL commands used to define the structure of a database.
DDL allows you to create, modify, and delete database objects such as tables, indexes, views, functions and schemas.
Example DDL commands include: *CREATE*, *ALTER* and *DROP*.

The content of migration scripts (recipes) is usually DDL commands. Besides subsequent recipes (migrations),
it is worth keeping the current state of the database in an organized structure of DDL scripts
in the code repository (e.g., GIT).

DBMigrator can automatically generate such scripts. DDL scripts are grouped according to a set of rules. 
The standard set creates the following structure:

- database.sql
- extensions.sql
- public/
    - schema.sql
    - fts.sql
    - index.sql
    - sequence.sql
    - ...
    - functions/
        - proc1.sql
        - proc2.sql
        - func1.sql
        - func2.sql
        - ...
    - types/
        - table1.sql
        - table2.sql
        - view1.sql
        - view2.sql
        - type1.sql
        - ...
- nextschema/
    - schema.sql
    - ...
- casts.sql
- ...
- unclassified.sql

DBMigrator internally uses the `pg_dump` tool from PostgreSQL.
This ensures that the DDL scripts are consistent with the current state and version of the database
and include all newly added functionalities to PostgreSQL.

#### Custom DDL ruleset

The above structure is suitable for our habits and our projects.
It will not always be the best solution, so you can develop your own set of rules.

You need to create a `pgsql_ddl_ruleset.yaml` file, which will be an array of rules:

| Field           | Type   | Description                                           | Default  |
|-----------------|--------|-------------------------------------------------------|----------|
| empty_namespace | bool   | Should the pg_dump entry have an empty namespace?     | `false`  |
| desc_pattern    | regexp | Pattern for pg_dump desc field (object type).         | `.*`     |
| tag_pattern     | regexp | Pattern for pg_dump tag field (object name, subtype). | `.*`     |
| filename        | string | Handlebars template of filename for the DDL script.   | required |

Elements that do not match any rule will be placed in the `unclassified.sql` file.

When creating rules for *ACL* and *COMMENT*, analyzing PostgreSQL code may be helpful:

* `parseAclItem` in `postgres/src/bin/pg_dump/dumputils.c`
* `_getObjectDescription` in `postgres/src/bin/pg_dump/pg_backup_archiver.c`

### Integartion with direnv

You can store your comfiguration in `.envrc` file and use [direnv](https://direnv.net) to load it.

`direnv` should be installed and then configure your shell to use it.

We recommend using `zsh` managed by [Zim framework](https://zimfw.sh/).

Example `.envrc` file:
```env
export DBMIGRATOR_DB_URL="postgresql://postgres@localhost/my-db"
export DBMIGRATOR_MIGRATIONS_PATH=$(expand_path my-db/migrations)
export DBMIGRATOR_DDL_PATH=$(expand_path my-db/ddl)
```

You can store DB password in `.pgpass` [password file](https://www.postgresql.org/docs/current/libpq-pgpass.html) 
in your home directory. 


## License

This project is licensed under the [MIT license](LICENSE-MIT) and [Apache license](LICENSE-APACHE-2.0).

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in dbmigrator by you, shall be licensed as MIT, without any additional
terms or conditions.

### Inspiration and ideas

- Katharina Fey (Refinery) - kookie@spacekookie.de
- João Oliveira (Refinery) - hello@jxs.pt
