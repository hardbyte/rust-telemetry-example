-- file: bookapp-dal/migrations/20250819210724_add_full_text_search.sql

-- Create a materialized view to store pre-computed search documents.
-- This view denormalizes text from works, authors, and series for efficient searching.
CREATE MATERIALIZED VIEW book_search_view AS
WITH work_authors_agg AS (
    -- Aggregate all author names for each work into a single string.
    SELECT
        wa.work_id,
        string_agg(a.name, ' ') AS author_names
    FROM work_authors wa
    JOIN authors a ON a.id = wa.author_id
    GROUP BY wa.work_id
),
work_series_agg AS (
    -- Aggregate all series names for each work.
    SELECT
        swa.work_id,
        string_agg(s.name, ' ') AS series_names
    FROM series_works_association swa
    JOIN series s ON s.id = swa.series_id
    GROUP BY swa.work_id
)
SELECT
    w.id AS work_id,
    -- Combine and weight the text fields to create the search document.
    -- Title is most important (A), series is next (B), authors are last (C).
    setweight(to_tsvector('english', coalesce(w.title, '')), 'A') ||
    setweight(to_tsvector('english', coalesce(waa.author_names, '')), 'C') ||
    setweight(to_tsvector('english', coalesce(wsa.series_names, '')), 'B')
    AS document
FROM
    works w
LEFT JOIN
    work_authors_agg waa ON w.id = waa.work_id
LEFT JOIN
    work_series_agg wsa ON w.id = wsa.work_id;

-- Create a unique index on the view's primary key.
-- This is REQUIRED to refresh the view CONCURRENTLY without locking.
CREATE UNIQUE INDEX idx_book_search_view_work_id ON book_search_view (work_id);

-- Create a GIN index on the 'document' tsvector column.
-- This is crucial for fast full-text search queries.
CREATE INDEX idx_book_search_view_document ON book_search_view USING GIN (document);