-- Extend events table with outbox fields for Kafka publishing
-- Fields:
--  - topic: optional Kafka topic name to publish to
--  - published_at: timestamp when the event was successfully published (NULL = pending)
--  - publish_attempts: number of publish attempts (default 0)
--  - publish_error: last publish error message (if any)

ALTER TABLE events
  ADD COLUMN IF NOT EXISTS topic TEXT;

ALTER TABLE events
  ADD COLUMN IF NOT EXISTS published_at TIMESTAMPTZ;

ALTER TABLE events
  ADD COLUMN IF NOT EXISTS publish_attempts INTEGER NOT NULL DEFAULT 0;

ALTER TABLE events
  ADD COLUMN IF NOT EXISTS publish_error TEXT;

-- Helpful indexes for outbox scanners

-- Fast scan for unpublished events in chronological order (stable ordering with id)
CREATE INDEX IF NOT EXISTS idx_events_outbox_unpublished
  ON events (occurred_at, id)
  WHERE published_at IS NULL;

-- Optional: filter by topic when scanning unpublished events
CREATE INDEX IF NOT EXISTS idx_events_outbox_topic_unpublished
  ON events (topic, occurred_at, id)
  WHERE published_at IS NULL;
