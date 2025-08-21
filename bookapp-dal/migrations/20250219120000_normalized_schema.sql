-- Normalized schema for book domain: authors, works, editions, series, associations, and generic events
-- This migration is additive and keeps the existing `books` table intact.

-- Helper function to maintain updated_at timestamps
create or replace function set_updated_at()
returns trigger as $$
begin
  new.updated_at = now();
  return new;
end;
$$ language plpgsql;

-- Authors: unique people or organizations credited for works (authors, editors, illustrators, etc.)
create table if not exists authors (
  id serial primary key,
  name text not null,
  sort_name text,
  created_at timestamp with time zone not null default now(),
  updated_at timestamp with time zone not null default now(),
  unique (name)
);

create trigger trg_authors_set_updated_at
before update on authors
for each row
execute function set_updated_at();

-- Works: conceptual works (e.g., "The Lord of the Rings"), not tied to specific ISBNs/editions
create table if not exists works (
  id serial primary key,
  title text not null,
  original_language text,
  description text,
  publication_year integer,
  created_at timestamp with time zone not null default now(),
  updated_at timestamp with time zone not null default now(),
  -- Soft uniqueness: allows same title in different publication years
  unique (title, publication_year)
);

create trigger trg_works_set_updated_at
before update on works
for each row
execute function set_updated_at();

-- Work-Authors many-to-many with optional role (Author, Editor, Illustrator, etc.)
create table if not exists work_authors (
  work_id integer not null,
  author_id integer not null,
  role text not null default 'Author',
  primary_author boolean not null default false,
  ord smallint,
  created_at timestamp with time zone not null default now(),
  updated_at timestamp with time zone not null default now(),
  constraint pk_work_authors primary key (work_id, author_id),
  constraint fk_work_authors_work_id foreign key (work_id) references works(id) on delete cascade,
  constraint fk_work_authors_author_id foreign key (author_id) references authors(id) on delete restrict
);

create index if not exists idx_work_authors_author_id on work_authors(author_id);
create index if not exists idx_work_authors_work_id on work_authors(work_id);

create trigger trg_work_authors_set_updated_at
before update on work_authors
for each row
execute function set_updated_at();

-- Editions: concrete publications tied to a work and ISBN
create table if not exists editions (
  id serial primary key,
  work_id integer not null,
  isbn text not null,
  title text, -- edition title override if different from work title
  publisher text,
  publication_date date,
  language text,
  page_count integer,
  format text, -- hardcover, paperback, ebook, etc.
  created_at timestamp with time zone not null default now(),
  updated_at timestamp with time zone not null default now(),
  constraint fk_editions_work_id foreign key (work_id) references works(id) on delete cascade,
  constraint uq_editions_isbn unique (isbn)
);

create index if not exists idx_editions_work_id on editions(work_id);

create trigger trg_editions_set_updated_at
before update on editions
for each row
execute function set_updated_at();

-- Series: collections of works (e.g., "The Wheel of Time")
create table if not exists series (
  id serial primary key,
  name text not null,
  description text,
  created_at timestamp with time zone not null default now(),
  updated_at timestamp with time zone not null default now(),
  constraint uq_series_name unique (name)
);

create trigger trg_series_set_updated_at
before update on series
for each row
execute function set_updated_at();

-- Series-Works association with ordering and a flag for the primary work identity if applicable
create table if not exists series_works_association (
  series_id integer not null,
  work_id integer not null,
  primary_work boolean not null default true,
  order_id integer,
  created_at timestamp with time zone not null default now(),
  updated_at timestamp with time zone not null default now(),
  constraint pk_series_works_association primary key (series_id, work_id),
  constraint fk_series_works_assoc_series_id foreign key (series_id) references series(id) on delete cascade,
  constraint fk_series_works_assoc_work_id foreign key (work_id) references works(id) on delete cascade
);

create index if not exists idx_series_works_assoc_work_id on series_works_association(work_id);

create trigger trg_series_works_assoc_set_updated_at
before update on series_works_association
for each row
execute function set_updated_at();

-- Generic events table for event logging, auditing, and outbox-style usage
create table if not exists events (
  id bigserial primary key,
  occurred_at timestamp with time zone not null default now(),
  aggregate_type text not null, -- e.g., 'work', 'edition', 'author', 'series'
  aggregate_id text not null,   -- stringly-typed to avoid cross-table coupling (could be UUID or integer as string)
  event_type text not null,     -- e.g., 'created', 'updated', 'deleted', 'status_changed'
  payload jsonb not null,       -- event data
  headers jsonb not null default '{}'::jsonb, -- optional metadata
  trace_id text,
  span_id text,
  source_service text,
  version integer not null default 1
);

create index if not exists idx_events_aggregate on events(aggregate_type, aggregate_id);
create index if not exists idx_events_occurred_at on events(occurred_at);
create index if not exists idx_events_event_type on events(event_type);
