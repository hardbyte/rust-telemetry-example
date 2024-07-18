-- Add migration script here
CREATE table books (
    id integer primary key autoincrement,
    title text,
    author text
);

insert into books (title, author) values ("The Name of the Wind", "Patrick Rothfus");
insert into books (title, author) values ("Dune", "Frank Herbert");

