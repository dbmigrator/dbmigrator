use pgarchive::TocEntry;
use serde::{Deserialize, Serialize};
use handlebars::Handlebars;

#[derive(Debug, Clone, Deserialize)]
pub struct PgDdlRule {
    #[serde(default)]
    pub empty_namespace: bool,
    pub desc_pattern: Option<String>,
    pub tag_pattern: Option<String>,
    pub filename: String,
}

#[derive(Debug, Clone, Serialize)]
struct TemplateData {
    namespace: String,
    desc_parts: Vec<String>,
    tag_parts: Vec<String>,
}

#[derive(Debug)]
struct PgDdlMatcher {
    empty_namespace: bool,
    desc_regex: regex::Regex,
    tag_regex: regex::Regex,
    filename_template: String,
}

impl PgDdlMatcher {
    fn new(handlebars: &mut Handlebars, rule: &PgDdlRule) -> Result<Self, regex::Error> {
        let desc_pattern = rule.desc_pattern.as_deref().unwrap_or(".*");
        let desc_pattern = desc_pattern.replace("{name}", r#"([[:word:]-]+|\"[[:word:]- ]+\")"#);
        let desc_regex = regex::Regex::new(&desc_pattern)?;
        let tag_pattern = rule.tag_pattern.as_deref().unwrap_or(".*");
        let tag_pattern = tag_pattern.replace("{name}", r#"([[:word:]-]+|\"[[:word:]- ]+\")"#);
        let tag_regex = regex::Regex::new(&tag_pattern)?;
        handlebars.register_template_string(&rule.filename, &rule.filename).unwrap();
        Ok(PgDdlMatcher {
            empty_namespace: rule.empty_namespace,
            desc_regex: desc_regex,
            tag_regex: tag_regex,
            filename_template: rule.filename.clone(),
        })
    }

    fn matches(&self, handlebars: &Handlebars, entry: &TocEntry) -> Result<Option<String>,handlebars::RenderError> {
        if self.empty_namespace != entry.namespace.is_empty() {
            return Ok(None);
        }
        if let Some(desc_captures) = self.desc_regex.captures(&entry.desc) {
            if let Some(tag_captures) = self.tag_regex.captures(&entry.tag) {
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
                return Ok(Some(handlebars.render(&self.filename_template, &data)?));
            }
        }
        Ok(None)
    }
}

#[derive(Debug)]
pub struct DdlConfig<'a> {
    handlebars: Handlebars<'a>,
    postgres_ddl_ruleset: Vec<PgDdlMatcher>,
}

impl<'a> DdlConfig<'a> {
    pub fn new() -> Self {
        DdlConfig {
            handlebars: Handlebars::new(),
            postgres_ddl_ruleset: Vec::new(),
        }
    }
    pub fn push_postgres_ddl_ruleset(&mut self, ruleset: Vec<PgDdlRule>) {
        for rule in ruleset.iter() {
            match PgDdlMatcher::new(&mut self.handlebars, rule) {
                Ok(matcher) => self.postgres_ddl_ruleset.push(matcher),
                Err(e) => eprintln!("Error compiling regex: {}", e),
            }
        }
    }

    pub fn pgddl_filename(&self, entry: &TocEntry) -> Option<String> {
        for rule in self.postgres_ddl_ruleset.iter() {
            match rule.matches(&self.handlebars, entry) {
                Ok(Some(filename)) => return Some(filename),
                Ok(None) => (),
                Err(e) => {
                    eprintln!("Error rendering template: {}", e);
                }
            }
        };
        None
    }
}
