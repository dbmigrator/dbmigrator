use std::path::Path;

use dbmigrator::{
    load_sql_recipes, simple_kind_detector, Migrator, RecipeScript, SIMPLE_FILENAME_PATTERN,
};

mod migrations {
    dbmigrator::embed_migrations!("../examples/mysql_mattermost_config");
}
#[test]
fn same_output() {
    let migrations = load("../examples/mysql_mattermost_config/");
    let macro_migrations = migrations::recipes();
    assert_eq!(migrations, macro_migrations);
}

fn load(path: &str) -> Vec<RecipeScript> {
    let files = dbmigrator::find_sql_files(Path::new(path).canonicalize().unwrap()).unwrap();
    let mut recipes = Vec::new();
    load_sql_recipes(
        &mut recipes,
        files,
        SIMPLE_FILENAME_PATTERN,
        Some(simple_kind_detector),
    )
    .unwrap();
    recipes
}
