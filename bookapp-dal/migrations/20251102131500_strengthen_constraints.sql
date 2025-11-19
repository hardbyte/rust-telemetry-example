-- Hardening constraints across the normalized schema to prevent inconsistent data.

ALTER TABLE authors
    ADD CONSTRAINT authors_name_not_empty CHECK (btrim(name) <> '');

ALTER TABLE works
    ADD CONSTRAINT works_title_not_empty CHECK (btrim(title) <> '');

ALTER TABLE series
    ADD CONSTRAINT series_name_not_empty CHECK (btrim(name) <> '');

ALTER TABLE editions
    ADD CONSTRAINT editions_isbn_not_empty CHECK (btrim(isbn) <> '');

ALTER TABLE editions
    ADD CONSTRAINT editions_page_count_positive CHECK (page_count IS NULL OR page_count > 0);

ALTER TABLE work_authors
    ADD CONSTRAINT work_authors_ord_positive CHECK (ord IS NULL OR ord > 0);

CREATE UNIQUE INDEX IF NOT EXISTS idx_work_authors_primary_unique
    ON work_authors (work_id)
    WHERE primary_author = true;
