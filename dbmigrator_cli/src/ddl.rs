use pgarchive::TocEntry;
use regex::Regex;
use serde::{Deserialize, Serialize};
use handlebars::Handlebars;

#[derive(Debug, Clone, Deserialize)]
pub struct DdlConfig {
    pub postgres_ddl_ruleset: Vec<PgDdlRule>,
}

#[derive(Debug, Clone, Serialize)]
struct TemplateData {
    namespace: String,
    desc_parts: Vec<String>,
    tag_parts: Vec<String>,
}

impl DdlConfig {
    pub fn pgddl_filename(&self, entry: &TocEntry) -> Option<String> {
        for rule in self.postgres_ddl_ruleset.iter() {
            if rule.empty_namespace != entry.namespace.is_empty() {
                continue;
            }
            let desc_regex = regex::Regex::new(rule.desc_pattern.as_deref().unwrap_or(".*")).unwrap();
            let tag_regex = regex::Regex::new(rule.tag_pattern.as_deref().unwrap_or(".*")).unwrap();
            if let Some(desc_captures) = desc_regex.captures(&entry.desc) {
                if let Some(tag_captures) = tag_regex.captures(&entry.tag) {
                    let data = TemplateData {
                        namespace: entry.namespace.clone(),
                        desc_parts: desc_captures
                            .iter()
                            .map(|m| m.unwrap().as_str().to_string())
                            .collect(),
                        tag_parts: tag_captures
                            .iter()
                            .map(|m| m.unwrap().as_str().to_string())
                            .collect(),
                    };
                    let mut handlebars = Handlebars::new();
                    handlebars.register_template_string("file", &rule.filename).unwrap();

                    match handlebars.render("file", &data) {
                        Ok(filename) => return Some(filename),
                        Err(e) => {
                            eprintln!("Error rendering template: {}", e);
                            return Some("error.sql".to_string());
                        },
                    }
                }
            }
        };
        None
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PgDdlRule {
    #[serde(default)]
    pub empty_namespace: bool,
    pub desc_pattern: Option<String>,
    pub tag_pattern: Option<String>,
    pub filename: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefKey {
    pub namespace: Option<String>,
    pub kind: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DefItem {
    pub key: DefKey,
    pub sub_kind: Option<String>,
    pub sub_name: Option<String>,
    pub sql_create: String,
    pub sql_drop: String,
}

impl DefItem {
    pub fn from_toc_entry(entry: &TocEntry) -> Self {
        let namespace = if entry.namespace.is_empty() {
            None
        } else {
            Some(entry.namespace.to_string())
        };
        let mut tag_parts = entry.tag.splitn(2, ' ');
        let tag_part1 = tag_parts.next().unwrap_or(&"");
        let tag_part2 = tag_parts.next().unwrap_or(&"");

        let kind;
        let name;
        let sub_kind;
        let sub_name;

        if matches!(entry.desc.as_str(), "ACL" | "COMMENT") {
            if tag_part1 == "COLUMN" {
                let mut col_parts = tag_part2.splitn(2, '.');
                kind = "TABLE".to_string();
                name = Some(col_parts.next().unwrap_or(&"").to_string());
                sub_kind = Some(format!("{} {}", entry.desc.as_str(), "COLUMN").to_string());
                sub_name = Some(col_parts.next().unwrap_or(&"").to_string());
            } else {
                // FIXME: FOREIGN DATA WRAPPER dummy
                // FIXME: FOREIGN SERVER s1
                // TEXT SEARCH PARSER alt_ts_prs1
                // TEXT SEARCH TEMPLATE alt_ts_temp1
                // TEXT SEARCH DICTIONARY alt_ts_dict1
                // TEXT SEARCH CONFIGURATION alt_ts_conf1
                kind = tag_part1.to_string();
                name = Some(tag_part2.to_string());
                sub_kind = Some(entry.desc.clone());
                sub_name = None;
            }
        } else if matches!(
            entry.desc.as_str(),
            "CONSTRAINT"
                | "DEFAULT"
                | "FK CONSTRAINT"
                | "POLICY"
                | "ROW SECURITY"
                | "RULE"
                | "TRIGGER"
        ) {
            kind = "TABLE".to_string();
            name = Some(tag_part1.to_string());
            sub_kind = Some(entry.desc.clone());
            sub_name = Some(tag_part2.to_string());
        } else if matches!(
            entry.desc.as_str(),
            "ACCESS METHOD"
                | "CAST"
                | "DEFAULT ACL"
                | "ENCODING"
                | "EVENT TRIGGER"
                | "FOREIGN DATA WRAPPER"
                | "PROCEDURAL LANGUAGE"
            | "SCHEMA"
            | "SEARCHPATH"
        ) {
            kind = entry.desc.clone();
            name = None;
            sub_kind = None;
            sub_name = Some(entry.tag.clone());
        } else {
            kind = entry.desc.clone();
            name = Some(tag_part1.to_string());
            sub_kind = None;
            sub_name = Some(tag_part2.to_string());
        };
        Self {
            key: DefKey {
                namespace,
                kind,
                name,
            },
            sub_kind,
            sub_name,
            sql_create: entry.defn.to_string(),
            sql_drop: entry.drop_stmt.to_string(),
        }
    }

    pub fn write_tab(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}",
            self.key.namespace.as_deref().unwrap_or(""),
            self.key.kind,
            self.key.name.as_deref().unwrap_or(""),
            self.sub_kind.as_deref().unwrap_or(""),
            self.sub_name.as_deref().unwrap_or(""),
        )
    }
}
