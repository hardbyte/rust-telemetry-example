-- file: bookapp-dal/migrations/20251110190000_add_work_authors_primary_index.sql

-- Composite index to accelerate queries that fetch the primary author for each work.
-- These lookups are performed on every /books page and in repository helpers, so we
-- ensure there is a selective covering index.
CREATE INDEX IF NOT EXISTS idx_work_authors_primary_author
    ON work_authors (work_id, author_id)
    WHERE primary_author = true;

