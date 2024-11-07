-- TODO: nontransactional=true
-- CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_poststats_userid ON poststats(userid)
CREATE INDEX IF NOT EXISTS idx_poststats_userid ON poststats(userid)
