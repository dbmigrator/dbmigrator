use crate::recipe::RecipeKind;
use std::fmt;
use std::str::FromStr;
use time::OffsetDateTime;

/// A migration changelog entry
#[derive(Clone, Debug)]
pub struct Changelog {
    log_id: i32,
    version: String,
    name: Option<String>,
    kind: String,
    checksum: Option<String>,
    apply_by: Option<String>,
    start_ts: Option<OffsetDateTime>,
    finish_ts: Option<OffsetDateTime>,
    revert_ts: Option<OffsetDateTime>,
}

impl Changelog {
    pub fn new(
        log_id: i32,
        version: String,
        name: Option<String>,
        kind: String,
        checksum: Option<String>,
        apply_by: Option<String>,
        start_ts: Option<OffsetDateTime>,
        finish_ts: Option<OffsetDateTime>,
        revert_ts: Option<OffsetDateTime>,
    ) -> Self {
        Changelog {
            log_id,
            version,
            name,
            kind,
            checksum,
            apply_by,
            start_ts,
            finish_ts,
            revert_ts,
        }
    }

    pub fn log_id(&self) -> i32 {
        self.log_id
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn kind(&self) -> Option<RecipeKind> {
        RecipeKind::from_str(&self.kind).ok()
    }

    pub fn is_baseline(&self) -> bool {
        self.kind == RecipeKind::Baseline.to_string()
    }

    pub fn is_upgrade(&self) -> bool {
        self.kind == RecipeKind::Upgrade.to_string()
    }

    pub fn is_fix(&self) -> bool {
        self.kind == RecipeKind::Revert.to_string() || self.kind == RecipeKind::Fixup.to_string()
    }

    pub fn kind_str(&self) -> &str {
        &self.kind
    }

    pub fn checksum(&self) -> Option<&str> {
        self.checksum.as_deref()
    }

    pub fn checksum32(&self) -> Option<&str> {
        match self.checksum {
            Some(ref c) => Some(&c[0..8]),
            None => None,
        }
    }

    pub fn apply_by(&self) -> Option<&str> {
        self.apply_by.as_deref()
    }

    pub fn start_ts(&self) -> Option<OffsetDateTime> {
        self.start_ts
    }

    pub fn finish_ts(&self) -> Option<OffsetDateTime> {
        self.finish_ts
    }

    pub fn revert_ts(&self) -> Option<OffsetDateTime> {
        self.revert_ts
    }

    pub fn set_start_ts(&mut self, start_ts: Option<OffsetDateTime>) {
        self.start_ts = start_ts;
    }

    pub fn set_finish_ts(&mut self, finish_ts: Option<OffsetDateTime>) {
        self.finish_ts = finish_ts;
    }

    pub fn set_revert_ts(&mut self, revert_ts: Option<OffsetDateTime>) {
        self.revert_ts = revert_ts;
    }
}

impl fmt::Display for Changelog {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "#{} v: {} {} {}, ({})",
            self.log_id,
            self.version,
            match self.name {
                Some(ref n) => n,
                None => "-",
            },
            self.kind,
            match self.checksum {
                Some(ref c) => c,
                None => "-",
            },
        )?;
        if let Some(ref start_ts) = self.start_ts {
            write!(f, ", started: {:?}", start_ts)?;
        }
        if let Some(ref finish_ts) = self.finish_ts {
            write!(f, ", finished: {:?}", finish_ts)?;
        }
        if let Some(ref revert_ts) = self.revert_ts {
            write!(f, ", reverted: {:?}", revert_ts)?;
        }
        Ok(())
    }
}
