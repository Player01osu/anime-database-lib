pub const IMPORTS: &str = r#"
    PRAGMA journal_mode = WAL;
    PRAGMA synchronous = normal;
    PRAGMA temp_store = memory;
    PRAGMA mmap_size = 30000000000;

    CREATE TABLE IF NOT EXISTS anime (
        anime TEXT NOT NULL,
        current_episode INT,
        current_season INT,
        last_watched INT,

        PRIMARY KEY (anime)
    );

    CREATE TABLE IF NOT EXISTS location (
        location TEXT PRIMARY KEY NOT NULL,
        anime TEXT NOT NULL,

        CONSTRAINT fk_anime
        FOREIGN KEY (anime)
        REFERENCES anime (anime)
    );

    CREATE TABLE IF NOT EXISTS episode (
        path TEXT PRIMARY KEY UNIQUE NOT NULL,
        anime TEXT NOT NULL,
        episode INT,
        season INT,
        special TEXT,

        CONSTRAINT fk_anime
        FOREIGN KEY (anime)
        REFERENCES anime (anime)
    );

    CREATE UNIQUE INDEX IF NOT EXISTS filename_idx
    ON anime(anime);

    CREATE INDEX IF NOT EXISTS episode_season_idx
    ON episode(episode, season);
"#;
