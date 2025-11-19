-- Switch domain tables to UUIDv7 primary keys and update foreign keys.
-- Requires pgcrypto for gen_random_bytes used in the helper.

CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE OR REPLACE FUNCTION uuid_v7()
RETURNS uuid
LANGUAGE plpgsql
AS $$
DECLARE
  unix_ms bigint;
  unix_hex text;
  rand_hex text;
  bytes bytea;
BEGIN
  unix_ms := floor(extract(epoch FROM clock_timestamp()) * 1000);
  unix_hex := lpad(to_hex(unix_ms), 12, '0');
  rand_hex := encode(gen_random_bytes(10), 'hex');
  bytes := decode(unix_hex || rand_hex, 'hex');

  -- Embed UUIDv7 version and variant bits.
  bytes := set_byte(bytes, 6, (get_byte(bytes, 6) & 15) | 112); -- version 7
  bytes := set_byte(bytes, 8, (get_byte(bytes, 8) & 63) | 128); -- variant 1

  RETURN encode(bytes, 'hex')::uuid;
END;
$$;

-- Drop existing constraints that reference integer keys.
DROP INDEX IF EXISTS idx_book_search_view_document;
DROP INDEX IF EXISTS idx_book_search_view_work_id;
DROP MATERIALIZED VIEW IF EXISTS book_search_view;

ALTER TABLE work_authors DROP CONSTRAINT IF EXISTS fk_work_authors_work_id;
ALTER TABLE work_authors DROP CONSTRAINT IF EXISTS fk_work_authors_author_id;
ALTER TABLE work_authors DROP CONSTRAINT IF EXISTS pk_work_authors;

ALTER TABLE series_works_association DROP CONSTRAINT IF EXISTS fk_series_works_assoc_series_id;
ALTER TABLE series_works_association DROP CONSTRAINT IF EXISTS fk_series_works_assoc_work_id;
ALTER TABLE series_works_association DROP CONSTRAINT IF EXISTS pk_series_works_association;

ALTER TABLE editions DROP CONSTRAINT IF EXISTS fk_editions_work_id;

ALTER TABLE authors DROP CONSTRAINT IF EXISTS authors_pkey;
ALTER TABLE works DROP CONSTRAINT IF EXISTS works_pkey;
ALTER TABLE editions DROP CONSTRAINT IF EXISTS editions_pkey;
ALTER TABLE series DROP CONSTRAINT IF EXISTS series_pkey;
ALTER TABLE books DROP CONSTRAINT IF EXISTS books_pkey;

-- Add UUID columns to owning tables.
ALTER TABLE authors ADD COLUMN IF NOT EXISTS id_v7 uuid DEFAULT uuid_v7();
UPDATE authors SET id_v7 = uuid_v7() WHERE id_v7 IS NULL;
ALTER TABLE authors ALTER COLUMN id_v7 SET NOT NULL;

ALTER TABLE works ADD COLUMN IF NOT EXISTS id_v7 uuid DEFAULT uuid_v7();
UPDATE works SET id_v7 = uuid_v7() WHERE id_v7 IS NULL;
ALTER TABLE works ALTER COLUMN id_v7 SET NOT NULL;

ALTER TABLE editions ADD COLUMN IF NOT EXISTS id_v7 uuid DEFAULT uuid_v7();
UPDATE editions SET id_v7 = uuid_v7() WHERE id_v7 IS NULL;
ALTER TABLE editions ALTER COLUMN id_v7 SET NOT NULL;

ALTER TABLE series ADD COLUMN IF NOT EXISTS id_v7 uuid DEFAULT uuid_v7();
UPDATE series SET id_v7 = uuid_v7() WHERE id_v7 IS NULL;
ALTER TABLE series ALTER COLUMN id_v7 SET NOT NULL;

ALTER TABLE books ADD COLUMN IF NOT EXISTS id_v7 uuid DEFAULT uuid_v7();
UPDATE books SET id_v7 = uuid_v7() WHERE id_v7 IS NULL;
ALTER TABLE books ALTER COLUMN id_v7 SET NOT NULL;

-- Add UUID foreign key columns and populate from mappings.
ALTER TABLE work_authors ADD COLUMN IF NOT EXISTS work_id_v7 uuid;
UPDATE work_authors wa
SET work_id_v7 = w.id_v7
FROM works w
WHERE wa.work_id = w.id;
ALTER TABLE work_authors ALTER COLUMN work_id_v7 SET NOT NULL;

ALTER TABLE work_authors ADD COLUMN IF NOT EXISTS author_id_v7 uuid;
UPDATE work_authors wa
SET author_id_v7 = a.id_v7
FROM authors a
WHERE wa.author_id = a.id;
ALTER TABLE work_authors ALTER COLUMN author_id_v7 SET NOT NULL;

ALTER TABLE editions ADD COLUMN IF NOT EXISTS work_id_v7 uuid;
UPDATE editions e
SET work_id_v7 = w.id_v7
FROM works w
WHERE e.work_id = w.id;
ALTER TABLE editions ALTER COLUMN work_id_v7 SET NOT NULL;

ALTER TABLE series_works_association ADD COLUMN IF NOT EXISTS series_id_v7 uuid;
UPDATE series_works_association swa
SET series_id_v7 = s.id_v7
FROM series s
WHERE swa.series_id = s.id;
ALTER TABLE series_works_association ALTER COLUMN series_id_v7 SET NOT NULL;

ALTER TABLE series_works_association ADD COLUMN IF NOT EXISTS work_id_v7 uuid;
UPDATE series_works_association swa
SET work_id_v7 = w.id_v7
FROM works w
WHERE swa.work_id = w.id;
ALTER TABLE series_works_association ALTER COLUMN work_id_v7 SET NOT NULL;

-- Drop legacy integer columns and rename UUID columns into place.
ALTER TABLE authors DROP COLUMN IF EXISTS id;
ALTER TABLE authors RENAME COLUMN id_v7 TO id;
ALTER TABLE authors ALTER COLUMN id SET DEFAULT uuid_v7();

ALTER TABLE works DROP COLUMN IF EXISTS id;
ALTER TABLE works RENAME COLUMN id_v7 TO id;
ALTER TABLE works ALTER COLUMN id SET DEFAULT uuid_v7();

ALTER TABLE editions DROP COLUMN IF EXISTS id;
ALTER TABLE editions RENAME COLUMN id_v7 TO id;
ALTER TABLE editions ALTER COLUMN id SET DEFAULT uuid_v7();

ALTER TABLE editions DROP COLUMN IF EXISTS work_id;
ALTER TABLE editions RENAME COLUMN work_id_v7 TO work_id;

ALTER TABLE work_authors DROP COLUMN IF EXISTS work_id;
ALTER TABLE work_authors RENAME COLUMN work_id_v7 TO work_id;

ALTER TABLE work_authors DROP COLUMN IF EXISTS author_id;
ALTER TABLE work_authors RENAME COLUMN author_id_v7 TO author_id;

ALTER TABLE series DROP COLUMN IF EXISTS id;
ALTER TABLE series RENAME COLUMN id_v7 TO id;
ALTER TABLE series ALTER COLUMN id SET DEFAULT uuid_v7();

ALTER TABLE series_works_association DROP COLUMN IF EXISTS series_id;
ALTER TABLE series_works_association RENAME COLUMN series_id_v7 TO series_id;

ALTER TABLE series_works_association DROP COLUMN IF EXISTS work_id;
ALTER TABLE series_works_association RENAME COLUMN work_id_v7 TO work_id;

ALTER TABLE books DROP COLUMN IF EXISTS id;
ALTER TABLE books RENAME COLUMN id_v7 TO id;
ALTER TABLE books ALTER COLUMN id SET DEFAULT uuid_v7();

-- Recreate constraints with UUID keys.
ALTER TABLE authors ADD CONSTRAINT authors_pkey PRIMARY KEY (id);
ALTER TABLE works ADD CONSTRAINT works_pkey PRIMARY KEY (id);
ALTER TABLE editions ADD CONSTRAINT editions_pkey PRIMARY KEY (id);
ALTER TABLE series ADD CONSTRAINT series_pkey PRIMARY KEY (id);
ALTER TABLE books ADD CONSTRAINT books_pkey PRIMARY KEY (id);

ALTER TABLE work_authors ADD CONSTRAINT pk_work_authors PRIMARY KEY (work_id, author_id);
ALTER TABLE work_authors ADD CONSTRAINT fk_work_authors_work_id FOREIGN KEY (work_id) REFERENCES works(id) ON DELETE CASCADE;
ALTER TABLE work_authors ADD CONSTRAINT fk_work_authors_author_id FOREIGN KEY (author_id) REFERENCES authors(id) ON DELETE CASCADE;

ALTER TABLE editions ADD CONSTRAINT fk_editions_work_id FOREIGN KEY (work_id) REFERENCES works(id) ON DELETE CASCADE;

ALTER TABLE series_works_association ADD CONSTRAINT pk_series_works_association PRIMARY KEY (series_id, work_id);
ALTER TABLE series_works_association ADD CONSTRAINT fk_series_works_assoc_series_id FOREIGN KEY (series_id) REFERENCES series(id) ON DELETE CASCADE;
ALTER TABLE series_works_association ADD CONSTRAINT fk_series_works_assoc_work_id FOREIGN KEY (work_id) REFERENCES works(id) ON DELETE CASCADE;

-- Recreate helpful indexes on the new UUID columns.
DROP INDEX IF EXISTS idx_work_authors_author_id;
DROP INDEX IF EXISTS idx_work_authors_work_id;
CREATE INDEX idx_work_authors_author_id ON work_authors(author_id);
CREATE INDEX idx_work_authors_work_id ON work_authors(work_id);

DROP INDEX IF EXISTS idx_editions_work_id;
CREATE INDEX idx_editions_work_id ON editions(work_id);

DROP INDEX IF EXISTS idx_series_works_assoc_work_id;
CREATE INDEX idx_series_works_assoc_work_id ON series_works_association(work_id);

-- Recreate materialized view and indexes with UUID identifiers.
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
)
SELECT
    w.id AS work_id,
    setweight(to_tsvector('english', coalesce(w.title, '')), 'A') ||
    setweight(to_tsvector('english', coalesce(waa.author_names, '')), 'C') ||
    setweight(to_tsvector('english', coalesce(wsa.series_names, '')), 'B') AS document
FROM
    works w
LEFT JOIN
    work_authors_agg waa ON w.id = waa.work_id
LEFT JOIN
    work_series_agg wsa ON w.id = wsa.work_id;

CREATE UNIQUE INDEX idx_book_search_view_work_id ON book_search_view (work_id);
CREATE INDEX idx_book_search_view_document ON book_search_view USING GIN (document);
