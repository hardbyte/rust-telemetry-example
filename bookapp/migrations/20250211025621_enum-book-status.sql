-- Add a custom enum type for book status
create type book_status as enum ('available', 'borrowed', 'lost');

-- CREATE table if not exists books (
--                                      id serial primary key,
--                                      title text,
--                                      author text
-- );

-- Add a column for book status
alter table books add column status book_status default 'available' not null;

