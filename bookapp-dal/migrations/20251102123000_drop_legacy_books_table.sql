-- Remove the legacy `books` table which previously stored denormalized data.
-- All book records now live in the normalized `works`/`authors` schema.
DROP TABLE IF EXISTS books;

-- Clean up the sequence that backed the old serial primary key.
DROP SEQUENCE IF EXISTS books_id_seq;
