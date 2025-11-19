-- Restructure the search view to include display columns and reduce join pressure.
-- Also introduce selective indexes for primary author lookups that power searches.

DROP INDEX IF EXISTS idx_book_search_view_document;
DROP INDEX IF EXISTS idx_book_search_view_work_id;
DROP MATERIALIZED VIEW IF EXISTS book_search_view;

CREATE MATERIALIZED VIEW book_search_view AS
WITH work_authors_agg AS (
    SELECT
        wa.work_id,
        string_agg(a.name, ' ') AS author_names
    FROM work_authors wa
    JOIN authors a ON a.id = wa.author_id
    GROUP BY wa.work_id
),
work_series_agg AS (
    SELECT
        swa.work_id,
        string_agg(s.name, ' ') AS series_names
    FROM series_works_association swa
    JOIN series s ON s.id = swa.series_id
    GROUP BY swa.work_id
),
primary_authors AS (
    SELECT
        wa.work_id,
        a.name AS primary_author_name
    FROM work_authors wa
    JOIN authors a ON a.id = wa.author_id
    WHERE wa.primary_author = true
)
SELECT
    w.id AS work_id,
    w.title AS work_title,
    COALESCE(pa.primary_author_name, 'Unknown Author') AS primary_author_name,
    setweight(to_tsvector('english', coalesce(w.title, '')), 'A') ||
    setweight(to_tsvector('english', coalesce(waa.author_names, '')), 'C') ||
    setweight(to_tsvector('english', coalesce(wsa.series_names, '')), 'B') AS document
FROM
    works w
LEFT JOIN
    work_authors_agg waa ON w.id = waa.work_id
LEFT JOIN
    work_series_agg wsa ON w.id = wsa.work_id
LEFT JOIN
    primary_authors pa ON w.id = pa.work_id;

CREATE UNIQUE INDEX idx_book_search_view_work_id ON book_search_view (work_id);
CREATE INDEX idx_book_search_view_document ON book_search_view USING GIN (document);

-- Selective and covering index for primary author lookups to avoid full scans.
CREATE INDEX IF NOT EXISTS idx_work_authors_primary_partial
    ON work_authors (work_id, author_id)
    WHERE primary_author = true;
