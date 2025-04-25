use regex::Regex;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use thiserror::Error;
use version_compare::Cmp;
use walkdir::{DirEntry, WalkDir};

/// An Error occurred during a migration cycle
#[derive(Debug, Error)]
pub enum RecipeError {
    #[error("invalid regex pattern")]
    InvalidRegex(regex::Error),

    #[error("invalid recipe script path `{path}`")]
    InvalidRecipePath {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("invalid recipe script file `{path}`")]
    InvalidRecipeFile {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("wrong filename format of recipe script `{file_stem}`")]
    InvalidFilename { file_stem: String },

    #[error("invalid recipe kind `{kind}`")]
    InvalidRecipeKind { kind: String },

    #[error("versions `{version}` must be unique for upgrade/baseline recipe (check `{name1}` and `{name2}`)"
    )]
    RepeatedVersion {
        version: String,
        name1: String,
        name2: String,
    },

    #[error("old_checksum metadata is required for revert recipe `{version}` `{name}` - ")]
    InvalidRevertMeta { version: String, name: String },

    #[error("old_checksum, new_name and new_checksum metadata are required for fixup recipe `{version}` `{name}`"
    )]
    InvalidFixupMeta { version: String, name: String },

    #[error("fixup `{version} {name}` cannot refer to existing recipe `{old_checksum}`")]
    ConflictedFixup {
        version: String,
        name: String,
        old_checksum: String,
    },

    #[error("unknown target `{new_version} {new_name} ({new_checksum})` in fixup migration `{version} {name}` for {old_checksum}`"
    )]
    InvalidFixupNewTarget {
        version: String,
        name: String,
        old_checksum: String,
        new_version: String,
        new_name: String,
        new_checksum: String,
    },
}

#[derive(Ord, PartialOrd, Eq, PartialEq, Clone, Debug)]
pub enum RecipeKind {
    Baseline,
    Upgrade,
    Revert,
    Fixup,
}

impl FromStr for RecipeKind {
    type Err = RecipeError;

    fn from_str(s: &str) -> Result<RecipeKind, RecipeError> {
        match s {
            "baseline" => Ok(RecipeKind::Baseline),
            "upgrade" => Ok(RecipeKind::Upgrade),
            "revert" => Ok(RecipeKind::Revert),
            "fixup" => Ok(RecipeKind::Fixup),
            _ => Err(RecipeError::InvalidRecipeKind { kind: s.into() }),
        }
    }
}

impl std::fmt::Display for RecipeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecipeKind::Baseline => write!(f, "baseline"),
            RecipeKind::Upgrade => write!(f, "upgrade"),
            RecipeKind::Revert => write!(f, "revert"),
            RecipeKind::Fixup => write!(f, "fixup"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecipeMeta {
    Baseline,
    Upgrade,
    Revert {
        old_checksum: Cow<'static, str>,
        maximum_version: Cow<'static, str>,
    },
    Fixup {
        old_checksum: Cow<'static, str>,
        maximum_version: Cow<'static, str>,
        new_version: Cow<'static, str>,
        new_name: Cow<'static, str>,
        new_checksum: Cow<'static, str>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecipeScript {
    pub version: Cow<'static, str>,
    pub name: Cow<'static, str>,
    pub checksum: Cow<'static, str>,
    pub sql: Cow<'static, str>,
    pub meta: RecipeMeta,
}

impl RecipeScript {
    pub fn new(
        version: Cow<'static, str>,
        name: Cow<'static, str>,
        sql: Cow<'static, str>,
        default_kind: Option<RecipeKind>,
    ) -> Result<RecipeScript, RecipeError> {
        let mut hasher = Sha256::new();
        hasher.update(&*sql);

        let checksum = format!("{:x}", hasher.finalize());

        let mut metadata = HashMap::new();
        parse_sql_metadata(&sql, &mut metadata);

        let mut version = version;
        if let Some(meta_version) = metadata.get("version") {
            version = Cow::Owned(meta_version.to_owned());
        }

        let mut name = name;
        if let Some(meta_name) = metadata.get("name") {
            name = Cow::Owned(meta_name.to_owned());
        }

        let mut kind = default_kind;
        if let Some(meta_kind) = metadata.get("kind") {
            kind = Some(RecipeKind::from_str(meta_kind)?);
        }

        let meta = match kind {
            Some(RecipeKind::Baseline) => RecipeMeta::Baseline,
            Some(RecipeKind::Upgrade) => RecipeMeta::Upgrade,
            Some(RecipeKind::Revert) => {
                if let Some(old_checksum) = metadata.get("old_checksum") {
                    let maximum_version = metadata
                        .get("maximum_version")
                        .map(String::as_str)
                        .unwrap_or(&version)
                        .to_owned();
                    RecipeMeta::Revert {
                        old_checksum: Cow::Owned(old_checksum.clone()),
                        maximum_version: Cow::Owned(maximum_version),
                    }
                } else {
                    return Err(RecipeError::InvalidRevertMeta {
                        version: (*version).to_owned(),
                        name: (*name).to_owned(),
                    });
                }
            }
            Some(RecipeKind::Fixup) => {
                if let (Some(old_checksum), Some(new_name), Some(new_checksum)) = (
                    metadata.get("old_checksum"),
                    metadata.get("new_name"),
                    metadata.get("new_checksum"),
                ) {
                    let maximum_version = metadata
                        .get("maximum_version")
                        .map(String::as_str)
                        .unwrap_or(&version)
                        .to_owned();
                    let new_version = metadata
                        .get("new_version")
                        .map(String::as_str)
                        .unwrap_or(&version)
                        .to_owned();
                    RecipeMeta::Fixup {
                        old_checksum: Cow::Owned(old_checksum.clone()),
                        maximum_version: Cow::Owned(maximum_version),
                        new_version: Cow::Owned(new_version),
                        new_name: Cow::Owned(new_name.clone()),
                        new_checksum: Cow::Owned(new_checksum.clone()),
                    }
                } else {
                    return Err(RecipeError::InvalidFixupMeta {
                        version: (*version).to_owned(),
                        name: (*name).to_owned(),
                    });
                }
            }
            _ => {
                return Err(RecipeError::InvalidRecipeKind {
                    kind: "unknown".to_string(),
                });
            }
        };

        Ok(RecipeScript {
            version,
            name,
            checksum: Cow::Owned(checksum),
            sql,
            meta,
        })
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn sql(&self) -> &str {
        &self.sql
    }

    pub fn kind(&self) -> RecipeKind {
        match &self.meta {
            RecipeMeta::Baseline => RecipeKind::Baseline,
            RecipeMeta::Upgrade => RecipeKind::Upgrade,
            RecipeMeta::Revert { .. } => RecipeKind::Revert,
            RecipeMeta::Fixup { .. } => RecipeKind::Fixup,
        }
    }

    pub fn is_baseline(&self) -> bool {
        matches!(self.meta, RecipeMeta::Baseline)
    }

    pub fn is_upgrade(&self) -> bool {
        matches!(self.meta, RecipeMeta::Upgrade)
    }

    pub fn match_checksum(&self, checksum: &str) -> bool {
        // The minimum length of a checksum pattern is 8.
        if checksum.len() < 8 {
            return false;
        }
        self.checksum.starts_with(checksum)
    }
    pub fn checksum(&self) -> &str {
        &self.checksum
    }

    pub fn checksum32(&self) -> &str {
        &self.checksum[0..8]
    }

    pub fn old_checksum(&self) -> Option<&str> {
        match &self.meta {
            RecipeMeta::Revert { old_checksum, .. } => Some(old_checksum),
            RecipeMeta::Fixup { old_checksum, .. } => Some(old_checksum),
            _ => None,
        }
    }

    pub fn old_checksum32(&self) -> Option<&str> {
        match &self.meta {
            RecipeMeta::Revert { old_checksum, .. } => Some(&old_checksum[0..8]),
            RecipeMeta::Fixup { old_checksum, .. } => Some(&old_checksum[0..8]),
            _ => None,
        }
    }

    pub fn maximum_version(&self) -> Option<&str> {
        match &self.meta {
            RecipeMeta::Revert {
                maximum_version, ..
            } => Some(maximum_version),
            RecipeMeta::Fixup {
                maximum_version, ..
            } => Some(maximum_version),
            _ => None,
        }
    }

    pub fn new_version(&self) -> Option<&str> {
        match &self.meta {
            RecipeMeta::Fixup { new_version, .. } => Some(new_version),
            _ => None,
        }
    }

    pub fn new_target(&self) -> Option<(&str, &str, &str)> {
        match &self.meta {
            RecipeMeta::Fixup {
                new_version,
                new_name,
                new_checksum,
                ..
            } => Some((&new_version, new_name, new_checksum)),
            _ => None,
        }
    }

    pub fn new_checksum32(&self) -> Option<&str> {
        match &self.meta {
            RecipeMeta::Fixup { new_checksum, .. } => Some(&new_checksum[0..8]),
            _ => None,
        }
    }
}

impl std::fmt::Display for RecipeScript {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            fmt,
            "{}{} {} ({})",
            self.version,
            if let Some(new_version) = self.new_version() {
                if new_version != self.version {
                    format!(" -> {}", new_version)
                } else {
                    "".to_string()
                }
            } else {
                "".to_string()
            },
            self.name,
            self.checksum32()
        )
    }
}

fn parse_sql_metadata(sql: &str, metadata: &mut HashMap<String, String>) {
    for line in sql.lines() {
        if !line.starts_with("--") {
            break;
        }
        let parts: Vec<&str> = line[2..].splitn(2, ':').collect();
        if parts.len() == 2 {
            metadata.insert(parts[0].trim().to_string(), parts[1].trim().to_string());
        }
    }
}

/// Find SQLs on file system recursively across given a location
pub fn find_sql_files(
    location: impl AsRef<Path>,
) -> Result<impl Iterator<Item = PathBuf>, RecipeError> {
    let location: &Path = location.as_ref();
    let location = location
        .canonicalize()
        .map_err(|err| RecipeError::InvalidRecipePath {
            path: location.to_path_buf(),
            source: err,
        })?;

    let file_paths = WalkDir::new(location)
        .into_iter()
        .filter_map(Result::ok)
        .map(DirEntry::into_path)
        .filter(|entry| {
            entry.is_file()
                && match entry.extension() {
                    Some(ext) => ext == OsStr::new("sql"),
                    None => false,
                }
        });

    Ok(file_paths)
}

/// Simple regex pattern for `{version}_{name}.sql` filename naming convention.
///
/// The version part must be alphanumeric with optional dots and dashes.
/// For example, `1.0.0-001`, `20240201T1123`, `00001`.
///
/// The name part must be alphanumeric with optional dots, dashes, and underscores.
/// For example, `create_user_table`, `add_email_column`, `issue_feature`.
pub static SIMPLE_FILENAME_PATTERN: &str = r"^([[:alnum:].\-]+)_([[:alnum:]._\-]+)$";

/// Simple recipe kind detector, allowing to determine the type of recipe
/// using the recipe name.
pub fn simple_kind_detector(_path: &Path, name: &str) -> Option<RecipeKind> {
    if name.starts_with("baseline") {
        Some(RecipeKind::Baseline)
    } else if name.starts_with("revert") {
        Some(RecipeKind::Revert)
    } else if name.starts_with("fixup") {
        Some(RecipeKind::Fixup)
    } else {
        Some(RecipeKind::Upgrade)
    }
}

/// Default comparator for recipe versions. Usually requires fixed size of version parts.
pub fn simple_compare(a: &str, b: &str) -> std::cmp::Ordering {
    a.cmp(&b)
}

/// Compare two versions using the `version_compare` crate.
/// Allow semver naming conventions.
///
/// For example, 1.0.0, 5.0.0, 5.3.0, 10.2.3, 10.10.1 will maintain the appropriate order.
pub fn version_compare(a: &str, b: &str) -> std::cmp::Ordering {
    let a = version_compare::Version::from(a);
    let b = version_compare::Version::from(b);
    match (a, b) {
        (Some(l), Some(r)) => match l.compare(r) {
            Cmp::Lt | Cmp::Le => std::cmp::Ordering::Less,
            version_compare::Cmp::Eq => std::cmp::Ordering::Equal,
            version_compare::Cmp::Gt | Cmp::Ge | Cmp::Ne => std::cmp::Ordering::Greater,
        },
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

/// Loads SQL recipes from a path. This enables dynamic migration discovery, as opposed to
/// embedding.
pub fn load_sql_recipes_iter(
    file_paths: impl Iterator<Item = PathBuf>,
    filename_pattern: &str,
    kind_detector: Option<fn(&Path, &str) -> Option<RecipeKind>>,
) -> Result<impl Iterator<Item = Result<(PathBuf, RecipeScript), RecipeError>>, RecipeError> {
    let regex = Regex::new(filename_pattern).map_err(|e| RecipeError::InvalidRegex(e))?;
    Ok(RecipeLoadIter {
        inner: file_paths,
        regex,
        kind_detector,
    })
}

/// Loads SQL recipes from a path. This enables dynamic migration discovery, as opposed to
/// embedding.
pub fn load_sql_recipes(
    recipes: &mut Vec<RecipeScript>,
    file_paths: impl Iterator<Item = PathBuf>,
    filename_pattern: &str,
    kind_detector: Option<fn(&Path, &str) -> Option<RecipeKind>>,
) -> Result<(), RecipeError> {
    let iter = load_sql_recipes_iter(file_paths, filename_pattern, kind_detector)?;

    for res in iter {
        match res {
            Ok((_, recipe)) => recipes.push(recipe),
            Err(err) => return Err(err),
        }
    }

    Ok(())
}

struct RecipeLoadIter<I> {
    inner: I,
    regex: Regex,
    kind_detector: Option<fn(&Path, &str) -> Option<RecipeKind>>,
}

impl<I> RecipeLoadIter<I> {
    fn load(&self, path: PathBuf) -> Result<(PathBuf, RecipeScript), RecipeError> {
        let sql = std::fs::read_to_string(path.as_path()).map_err(|e| {
            let path = path.to_owned();
            match e.kind() {
                std::io::ErrorKind::NotFound => RecipeError::InvalidRecipePath { path, source: e },
                _ => RecipeError::InvalidRecipeFile { path, source: e },
            }
        })?;

        //safe to call unwrap as find_migration_filenames returns canonical paths
        match path
            .file_stem()
            .and_then(|os_str| os_str.to_os_string().into_string().ok())
        {
            Some(file_stem) => {
                let captures = self.regex.captures(&file_stem).ok_or_else(|| {
                    RecipeError::InvalidFilename {
                        file_stem: file_stem.clone(),
                    }
                })?;
                let version: String = captures
                    .get(1)
                    .ok_or_else(|| RecipeError::InvalidFilename {
                        file_stem: file_stem.clone(),
                    })?
                    .as_str()
                    .to_string();
                let name: String = captures
                    .get(2)
                    .ok_or_else(|| RecipeError::InvalidFilename {
                        file_stem: file_stem.clone(),
                    })?
                    .as_str()
                    .to_string();
                let kind = match self.kind_detector {
                    Some(kind_detector) => kind_detector(&path, &name),
                    None => None,
                };
                let migration = RecipeScript::new(version.into(), name.into(), sql.into(), kind)?;
                Ok((path, migration))
            }
            None => Err(RecipeError::InvalidRecipePath {
                path,
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid file name"),
            }),
        }
    }
}

impl<I: Iterator<Item = PathBuf>> Iterator for RecipeLoadIter<I> {
    type Item = Result<(PathBuf, RecipeScript), RecipeError>;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(path) = self.inner.next() else {
            return None;
        };
        Some(self.load(path))
    }
}

/// The recipe collection is ordered by version and verified.
pub fn order_recipes(
    recipes: &mut Vec<RecipeScript>,
    version_comparator: fn(&str, &str) -> Ordering,
) -> Result<(), RecipeError> {
    let sorter = |item: &RecipeScript, version: &str, kind: RecipeKind| {
        (version_comparator)(item.version(), version).then_with(|| item.kind().cmp(&kind))
    };

    recipes.sort_by(|a, b| (sorter)(a, b.version(), b.kind()));

    for chunk in recipes.chunk_by(|a, b| a.version() == b.version()) {
        let mut baseline: Option<&RecipeScript> = None;
        let mut upgrade: Option<&RecipeScript> = None;

        for item in chunk {
            if item.is_baseline() {
                // Check if there are no duplicate baseline recipes (only one per version).
                if let Some(baseline) = baseline {
                    return Err(RecipeError::RepeatedVersion {
                        version: item.version().to_string(),
                        name1: baseline.name().to_string(),
                        name2: item.name().to_string(),
                    });
                }
                baseline = Some(item);
            } else if item.is_upgrade() {
                // Check if there are no duplicate upgrade recipes (only one per version).
                if let Some(upgrade) = upgrade {
                    return Err(RecipeError::RepeatedVersion {
                        version: item.version().to_string(),
                        name1: upgrade.name().to_string(),
                        name2: item.name().to_string(),
                    });
                }
                upgrade = Some(item);
            }
        }
        for item in chunk {
            // Check if the revert/fixup script does not refer to an existing baseline or upgrade recipe.
            if let Some(old_checksum) = item.old_checksum() {
                if let Some(baseline) = baseline {
                    if baseline.match_checksum(old_checksum) {
                        return Err(RecipeError::ConflictedFixup {
                            version: item.version().to_string(),
                            name: item.name().to_string(),
                            old_checksum: old_checksum.to_string(),
                        });
                    }
                }
                if let Some(upgrade) = upgrade {
                    if upgrade.match_checksum(old_checksum) {
                        return Err(RecipeError::ConflictedFixup {
                            version: item.version().to_string(),
                            name: item.name().to_string(),
                            old_checksum: old_checksum.to_string(),
                        });
                    }
                }
                baseline = Some(item);
            }
        }
    }
    for item in recipes.iter() {
        // Check if fixup scripts target refer to existing upgrade scripts.
        if let Some((new_version, new_name, new_checksum)) = item.new_target() {
            if !match recipes.binary_search_by(|a| (sorter)(a, new_version, RecipeKind::Upgrade)) {
                Ok(index) => {
                    recipes[index].name() == new_name && recipes[index].checksum() == new_checksum
                }
                Err(_) => false,
            } {
                return Err(RecipeError::InvalidFixupNewTarget {
                    version: item.version().to_string(),
                    name: item.name().to_string(),
                    old_checksum: item.old_checksum().unwrap().to_string(),
                    new_version: new_version.to_string(),
                    new_name: new_name.to_string(),
                    new_checksum: new_checksum.to_string(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn test_kind_from_str() {
        assert_eq!(
            RecipeKind::from_str("baseline").unwrap(),
            RecipeKind::Baseline
        );
        assert_eq!(
            RecipeKind::from_str("upgrade").unwrap(),
            RecipeKind::Upgrade
        );
        assert_eq!(RecipeKind::from_str("revert").unwrap(), RecipeKind::Revert);
        assert_eq!(RecipeKind::from_str("fixup").unwrap(), RecipeKind::Fixup);
        assert!(RecipeKind::from_str("unknown").is_err());
    }

    #[test]
    fn test_parse_sql_metadata() {
        let sql = "-- version: 1.0.0\n-- name: test_migration\n-- kind: upgrade\n-- old_checksum: abc123af\n-- new_checksum: def456dd\n-- maximum_version: 2.0.0\n-- new_version: 1.1.0\n-- new_name: new_test_migration\n\nSELECT * FROM test;\n-- some: data\n-- Extra comment...";
        let mut metadata = HashMap::new();
        parse_sql_metadata(sql, &mut metadata);

        assert_eq!(metadata.get("version"), Some(&"1.0.0".to_string()));
        assert_eq!(metadata.get("name"), Some(&"test_migration".to_string()));
        assert_eq!(metadata.get("kind"), Some(&"upgrade".to_string()));
        assert_eq!(metadata.get("old_checksum"), Some(&"abc123af".to_string()));
        assert_eq!(metadata.get("new_checksum"), Some(&"def456dd".to_string()));
        assert_eq!(metadata.get("maximum_version"), Some(&"2.0.0".to_string()));
        assert_eq!(metadata.get("new_version"), Some(&"1.1.0".to_string()));
        assert_eq!(
            metadata.get("new_name"),
            Some(&"new_test_migration".to_string())
        );
        assert!(metadata.get("some").is_none());
    }

    #[test]
    fn test_parse_sql_metadata_with_no_metadata() {
        let sql = "SELECT * FROM test;";
        let mut metadata = HashMap::new();
        parse_sql_metadata(sql, &mut metadata);

        assert!(metadata.is_empty());
    }

    #[test]
    fn test_parse_sql_metadata_with_partial_metadata() {
        let sql =
            "-- version: 1.0.0\n-- name: test_migration\nSELECT * FROM test;\n-- wrong: metadata";
        let mut metadata = HashMap::new();
        parse_sql_metadata(sql, &mut metadata);

        assert_eq!(metadata.get("version"), Some(&"1.0.0".to_string()));
        assert_eq!(metadata.get("name"), Some(&"test_migration".to_string()));
        assert!(metadata.get("kind").is_none());
        assert_eq!(metadata.len(), 2)
    }

    #[test]
    fn test_simple_compare() {
        assert_eq!(
            simple_compare("20240201T112301", "20240201T112301"),
            std::cmp::Ordering::Equal
        );
        assert_eq!(
            simple_compare("20240201T112301", "20240202T112301"),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            simple_compare("20240201T112301B", "20240201T112301"),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn use_version_compare() {
        assert_eq!(version_compare("1.0.0", "1.0.0"), std::cmp::Ordering::Equal);
        assert_eq!(version_compare("2.0.0", "10.0.1"), std::cmp::Ordering::Less);
        assert_eq!(
            version_compare("1.0.0-14", "1.0.0-2"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(version_compare("1.0.0", "2.0.0"), std::cmp::Ordering::Less);
        assert_eq!(
            version_compare("2.0.0", "1.0.0"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            version_compare("1.0.0-revB", "1.0.0-revA"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            version_compare("1.20.4-m1", "1.100.2-m2"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn find_sql_files_badly_named_files() {
        let tmp_dir = TempDir::new().unwrap();
        let migrations_dir = tmp_dir.path().join("migrations");
        fs::create_dir(&migrations_dir).unwrap();
        let sql1 = migrations_dir.join("2024-01-01Z1212_first.sql");
        fs::create_dir(&sql1).unwrap();
        let sql2 = migrations_dir.join("3.0_upgrade_comment.txt");
        fs::File::create(sql2).unwrap();
        let sql3 = migrations_dir.join("_3.2_upgrade");
        fs::File::create(sql3).unwrap();
        let sql4 = migrations_dir.join("3.2revert.SQL");
        fs::File::create(sql4).unwrap();

        assert_eq!(find_sql_files(migrations_dir).unwrap().count(), 0);
    }

    #[test]
    fn find_sql_files_wrong_path() {
        assert!(find_sql_files(Path::new("wrong_path")).is_err());
    }

    #[test]
    fn find_sql_files_good_named() {
        let tmp_dir = TempDir::new().unwrap();
        let migrations_dir = tmp_dir.path().join("migrations");
        fs::create_dir(&migrations_dir).unwrap();
        let sql1 = migrations_dir.join("1.0.0_baseline.sql");
        fs::File::create(&sql1).unwrap();
        let sql2 = migrations_dir.join("1.1.0_upgrade_first.sql");
        fs::File::create(&sql2).unwrap();
        let sql5 = migrations_dir.join("2.0.1_upgrade_first.sql");
        fs::File::create(&sql5).unwrap();
        let sql6 = migrations_dir.join("2.0.2_upgrade_second.sql");
        fs::File::create(&sql6).unwrap();
        let sub_dir = migrations_dir.join("subfolder");
        fs::create_dir(&sub_dir).unwrap();
        let sql_ign1 = sub_dir.join("2.2.2_baseline_ignore");
        fs::File::create(&sql_ign1).unwrap();
        let sql7 = sub_dir.join("2.2.2_baseline.sql");
        fs::File::create(&sql7).unwrap();
        let sql4 = migrations_dir.join("1.2_upgrade_second.sql");
        fs::File::create(&sql4).unwrap();
        let sql3 = migrations_dir.join("1.2_baseline.sql");
        fs::File::create(&sql3).unwrap();
        let sql_ign2 = migrations_dir.join("2.2.2_baseline.txt");
        fs::File::create(&sql_ign2).unwrap();

        let mut mods: Vec<PathBuf> = find_sql_files(migrations_dir).unwrap().collect();
        mods.sort();
        assert_eq!(sql1.canonicalize().unwrap(), mods[0]);
        assert_eq!(sql2.canonicalize().unwrap(), mods[1]);
        assert_eq!(sql3.canonicalize().unwrap(), mods[2]);
        assert_eq!(sql4.canonicalize().unwrap(), mods[3]);
        assert_eq!(sql5.canonicalize().unwrap(), mods[4]);
        assert_eq!(sql6.canonicalize().unwrap(), mods[5]);
        assert_eq!(sql7.canonicalize().unwrap(), mods[6]);
        assert_eq!(mods.len(), 7);
    }

    #[test]
    fn use_load_sql_files_diesel() {
        let sql_files = find_sql_files("../examples/pgsql_diesel1").unwrap();

        let mut migration_scripts = Vec::new();
        load_sql_recipes(
            &mut migration_scripts,
            sql_files,
            SIMPLE_FILENAME_PATTERN,
            Some(simple_kind_detector),
        )
        .unwrap();
        for (index, script) in migration_scripts.iter().enumerate() {
            println!("{}: {}", index, script);
        }
        assert_eq!(migration_scripts.len(), 9);
        assert_eq!(
            migration_scripts
                .iter()
                .filter(|a| a.kind() == RecipeKind::Baseline)
                .count(),
            1
        );
        assert_eq!(
            migration_scripts
                .iter()
                .filter(|a| a.kind() == RecipeKind::Upgrade)
                .count(),
            8
        );
        assert_eq!(
            migration_scripts
                .iter()
                .filter(|a| a.kind() == RecipeKind::Revert)
                .count(),
            0
        );
        assert_eq!(
            migration_scripts
                .iter()
                .filter(|a| a.kind() == RecipeKind::Fixup)
                .count(),
            0
        );

        let sql_files = find_sql_files("../examples/pgsql_diesel2").unwrap();

        let mut migration_scripts = Vec::new();
        load_sql_recipes(
            &mut migration_scripts,
            sql_files,
            SIMPLE_FILENAME_PATTERN,
            Some(simple_kind_detector),
        )
        .unwrap();
        order_recipes(&mut migration_scripts, simple_compare).unwrap();

        assert_eq!(migration_scripts.len(), 21);
        assert_eq!(
            migration_scripts.iter().filter(|a| a.is_baseline()).count(),
            1
        );
        assert_eq!(
            migration_scripts.iter().filter(|a| a.is_upgrade()).count(),
            20
        );
    }

    #[test]
    fn use_load_sql_files_mattermost() {
        let sql_files = find_sql_files("../examples/pgsql_mattermost_channels").unwrap();

        let mut migration_scripts = Vec::new();
        load_sql_recipes(
            &mut migration_scripts,
            sql_files,
            SIMPLE_FILENAME_PATTERN,
            Some(simple_kind_detector),
        )
        .unwrap();
        order_recipes(&mut migration_scripts, simple_compare).unwrap();

        assert_eq!(migration_scripts.len(), 128);
        assert_eq!(
            migration_scripts
                .iter()
                .filter(|a| a.kind() == RecipeKind::Upgrade)
                .count(),
            126
        );
        assert_eq!(
            migration_scripts
                .iter()
                .filter(|a| a.kind() == RecipeKind::Revert)
                .count(),
            0
        );
        assert_eq!(
            migration_scripts
                .iter()
                .filter(|a| a.kind() == RecipeKind::Fixup)
                .count(),
            1
        );
    }
}
