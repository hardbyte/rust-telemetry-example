-- Introduce an incrementally updated search index table managed by backend workers.
-- This replaces the book_search_view materialized view and avoids expensive refreshes.

DROP MATERIALIZED VIEW IF EXISTS book_search_view;
DROP INDEX IF EXISTS idx_book_search_view_work_id;
DROP INDEX IF EXISTS idx_book_search_view_document;

CREATE TABLE IF NOT EXISTS book_search_index (
    work_id uuid PRIMARY KEY REFERENCES works(id) ON DELETE CASCADE,
    work_title text NOT NULL,
    primary_author_name text NOT NULL,
    author_names text,
    series_names text,
    document tsvector NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_book_search_index_document
    ON book_search_index
    USING GIN (document);

CREATE INDEX IF NOT EXISTS idx_book_search_index_primary_author
    ON book_search_index (primary_author_name);

WITH authors_agg AS (
    SELECT
        wa.work_id,
        string_agg(a.name, ' ' ORDER BY wa.ord) AS author_names
    FROM work_authors wa
    JOIN authors a ON a.id = wa.author_id
    GROUP BY wa.work_id
),
primary_author AS (
    SELECT DISTINCT ON (wa.work_id)
        wa.work_id,
        a.name AS primary_author_name
    FROM work_authors wa
    JOIN authors a ON a.id = wa.author_id
    ORDER BY wa.work_id, (NOT wa.primary_author), wa.ord
),
series_agg AS (
    SELECT
        swa.work_id,
        string_agg(s.name, ' ' ORDER BY s.name) AS series_names
    FROM series_works_association swa
    JOIN series s ON s.id = swa.series_id
    GROUP BY swa.work_id
)
INSERT INTO book_search_index (
    work_id,
    work_title,
    primary_author_name,
    author_names,
    series_names,
    document
)
SELECT
    w.id,
    w.title,
    COALESCE(pa.primary_author_name, w.title),
    aa.author_names,
    sa.series_names,
    setweight(to_tsvector('english', COALESCE(w.title, '')), 'A') ||
        setweight(to_tsvector('english', COALESCE(aa.author_names, '')), 'B') ||
        setweight(to_tsvector('english', COALESCE(sa.series_names, '')), 'C')
FROM works w
LEFT JOIN authors_agg aa ON aa.work_id = w.id
LEFT JOIN primary_author pa ON pa.work_id = w.id
LEFT JOIN series_agg sa ON sa.work_id = w.id
ON CONFLICT (work_id) DO UPDATE
SET
    work_title = EXCLUDED.work_title,
    primary_author_name = EXCLUDED.primary_author_name,
    author_names = EXCLUDED.author_names,
    series_names = EXCLUDED.series_names,
    document = EXCLUDED.document,
    updated_at = now();
