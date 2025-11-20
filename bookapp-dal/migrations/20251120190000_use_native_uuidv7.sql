-- Switch default UUID generators to PostgreSQL 18's native uuidv7().
-- This migration assumes pgcrypto is installed (already required earlier)
-- and that the cluster has been upgraded to PostgreSQL 18 or newer.

DO $$
BEGIN
    PERFORM 1
    FROM pg_proc
    WHERE proname = 'uuidv7';

    IF NOT FOUND THEN
        RAISE EXCEPTION 'uuidv7() is unavailable. Please upgrade PostgreSQL to 18+ before applying this migration.';
    END IF;
END;
$$;

ALTER TABLE authors ALTER COLUMN id SET DEFAULT uuidv7();
ALTER TABLE works ALTER COLUMN id SET DEFAULT uuidv7();
ALTER TABLE editions ALTER COLUMN id SET DEFAULT uuidv7();
ALTER TABLE series ALTER COLUMN id SET DEFAULT uuidv7();
DROP FUNCTION IF EXISTS uuid_v7();
