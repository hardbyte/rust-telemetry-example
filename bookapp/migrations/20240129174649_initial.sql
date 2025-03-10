CREATE table if not exists books (
    id serial primary key,
    title text not null,
    author text not null
);


insert into books (title, author) values
    ('The Name of the Wind', 'Patrick Rothfus'),
    ('Dune', 'Frank Herbert'),
    ('1984', 'George Orwell'),
    ('To Kill a Mockingbird', 'Harper Lee'),
    ('The Great Gatsby', 'F. Scott Fitzgerald'),
    ('One Hundred Years of Solitude', 'Gabriel García Márquez'),
    ('Brave New World', 'Aldous Huxley'),
    ('The Catcher in the Rye', 'J.D. Salinger'),
    ('The Hobbit', 'J.R.R. Tolkien'),
    ('Pride and Prejudice', 'Jane Austen'),
    ('The Da Vinci Code', 'Dan Brown'),
    ('The Alchemist', 'Paulo Coelho'),
    ('The Girl with the Dragon Tattoo', 'Stieg Larsson'),
    ('The Hunger Games', 'Suzanne Collins'),
    ('The Fault in Our Stars', 'John Green'),
    ('The Martian', 'Andy Weir'),
    ('Gone Girl', 'Gillian Flynn'),
    ('The Help', 'Kathryn Stockett'),
    ('The Kite Runner', 'Khaled Hosseini'),
    ('The Hitchhiker''s Guide to the Galaxy', 'Douglas Adams'),
    ('The Road', 'Cormac McCarthy'),
    ('The Goldfinch', 'Donna Tartt'),
    ('The Handmaid''s Tale', 'Margaret Atwood'),
    ('The Pillars of the Earth', 'Ken Follett'),
    ('The Silence of the Lambs', 'Thomas Harris'),
    ('The Giver', 'Lois Lowry'),
    ('The Book Thief', 'Markus Zusak'),
    ('The Lovely Bones', 'Alice Sebold'),
    ('The Maze Runner', 'James Dashner'),
    ('The Shining', 'Stephen King'),
    ('The Curious Incident of the Dog in the Night-Time', 'Mark Haddon'),
    ('The Time Traveler''s Wife', 'Audrey Niffenegger'),
    ('The Girl on the Train', 'Paula Hawkins'),
    ('The Immortal Life of Henrietta Lacks', 'Rebecca Skloot'),
    ('The Perks of Being a Wallflower', 'Stephen Chbosky'),
    ('The Graveyard Book', 'Neil Gaiman'),
    ('The Divergent Series', 'Veronica Roth'),
    ('The Poisonwood Bible', 'Barbara Kingsolver'),
    ('The Nightingale', 'Kristin Hannah'),
    ('The Orphan Master''s Son', 'Adam Johnson'),
    ('The Glass Castle', 'Jeannette Walls'),
    ('The Absolutely True Diary of a Part-Time Indian', 'Sherman Alexie'),
    ('The Curious Case of Benjamin Button', 'F. Scott Fitzgerald'),
    ('The Godfather', 'Mario Puzo'),
    ('The Unbearable Lightness of Being', 'Milan Kundera'),
    ('The Cider House Rules', 'John Irving'),
    ('The Bourne Identity', 'Robert Ludlum'),
    ('The Firm', 'John Grisham'),
    ('The Outsiders', 'S.E. Hinton'),
    ('The Last Book in the Universe', 'Rodman Philbrick'),
    ('The Thirteenth Tale', 'Diane Setterfield'),
    ('The Sweetness at the Bottom of the Pie', 'Alan Bradley'),
    ('The Guernsey Literary and Potato Peel Pie Society', 'Mary Ann Shaffer'),
    ('The Invention of Wings', 'Sue Monk Kidd'),
    ('The Rosie Project', 'Graeme Simsion'),
    ('The Secret Life of Bees', 'Sue Monk Kidd'),
    ('The Language of Flowers', 'Vanessa Diffenbaugh'),
    ('The Night Circus', 'Erin Morgenstern'),
    ('The Golem and the Jinni', 'Helene Wecker'),
    ('The Raven Boys', 'Maggie Stiefvater'),
    ('The Storied Life of A.J. Fikry', 'Gabrielle Zevin'),
    ('The Forgotten Garden', 'Kate Morton'),
    ('The Bone Clocks', 'David Mitchell'),
    ('The Historian', 'Elizabeth Kostova'),
    ('The Signature of All Things', 'Elizabeth Gilbert'),
    ('The Diving Bell and the Butterfly', 'Jean-Dominique Bauby'),
    ('The Lacuna', 'Barbara Kingsolver'),
    ('The Passage', 'Justin Cronin'),
    ('The Westing Game', 'Ellen Raskin'),
    ('The Elegance of the Hedgehog', 'Muriel Barbery'),
    ('The Interestings', 'Meg Wolitzer'),
    ('The Magicians', 'Lev Grossman'),
    ('The Miniaturist', 'Jessie Burton'),
    ('The Orphan Train', 'Christina Baker Kline'),
    ('The Paris Wife', 'Paula McLain'),
    ('The Physick Book of Deliverance Dane', 'Katherine Howe'),
    ('The Scorpio Races', 'Maggie Stiefvater'),
    ('The Shadow of the Wind', 'Carlos Ruiz Zafón'),
    ('The Snow Child', 'Eowyn Ivey'),
    ('The Tiger''s Wife', 'Téa Obreht'),
    ('The Vacationers', 'Emma Straub'),
    ('The Mischief of the Mistletoe', 'Lauren Willig'),
    ('The Warded Man', 'Peter V. Brett'),
    ('The Wildflowers', 'Richard Jefferies'),
    ('The Windfall', 'Diksha Basu'),
    ('The Witch''s Daughter', 'Paula Brackston'),
    ('The Wolf Wilder', 'Katherine Rundell'),
    ('The Yonahlossee Riding Camp for Girls', 'Anton DiSclafani'),
    ('The Zookeeper''s Wife', 'Diane Ackerman'),
    ('The 100-Year-Old Man Who Climbed Out the Window and Disappeared', 'Jonas Jonasson'),
    ('The 5th Wave', 'Rick Yancey'),
    ('The 7½ Deaths of Evelyn Hardcastle', 'Stuart Turton');