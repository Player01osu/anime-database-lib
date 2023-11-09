use std::{collections::BTreeSet, path::Path, thread, fs, str::FromStr};
use rusqlite::{Connection, params};
use walkdir::WalkDir;

use crate::{episode::Episode, imports::IMPORTS};

#[derive(Debug)]
pub struct Database {
    path: String,
    anime_directory: Vec<String>,
    connection: Connection,
}

impl Database {
    pub fn new(path: String, anime_directory: Vec<String>) -> Self {
        let mut join_thread = false;
        if !Path::new(&path).is_file() {
            join_thread = true;
        }

        let path_move = path.clone();
        let anime_directory_move = anime_directory.clone();
        let thread = thread::spawn(move || {
            let path = path_move;
            let anime_directory = anime_directory_move;
            let sqlite_conn = Connection::open(&path)
                .map_err(|e| eprintln!("Failed to connect to sqlite database: {e}"))
                .unwrap();
            sqlite_conn.execute_batch(IMPORTS).unwrap();
            let mut database_async = Self {
                path: path.clone(),
                anime_directory: anime_directory.clone(),
                connection: sqlite_conn,
            };
            database_async.update()
        });

        // Wait for thread if database has not been created yet.
        if join_thread {
            thread.join().unwrap();
        }

        let sqlite_conn = Connection::open(&path)
            .map_err(|e| eprintln!("Failed to connect to sqlite database: {e}"))
            .unwrap();
        Self {
            path: path.clone(),
            anime_directory: anime_directory.clone(),
            connection: sqlite_conn,
        }
    }

    pub fn write(&mut self) {
        unimplemented!()
    }

    pub fn update(&mut self) {
        let mut stmt_anime = self.connection
            .prepare_cached(
                r#"
            INSERT OR IGNORE INTO anime (anime)
            VALUES (?1)
        "#,
            )
            .unwrap();
        let mut stmt_location = self.connection
            .prepare_cached(
                r#"
            INSERT OR IGNORE INTO location (anime, location)
            VALUES (?1, ?2)
        "#,
            )
            .unwrap();

        let list = self
            .anime_directory
            .iter()
            .filter_map(|v| fs::read_dir(v).ok())
            .flat_map(|v| v.map(|d| d.unwrap().path()));
        for i in list {
            stmt_anime
                .execute(params![i.file_name().unwrap().to_string_lossy(),])
                .unwrap();

            stmt_location
                .execute(params![
                    i.file_name().unwrap().to_string_lossy(),
                    i.to_string_lossy()
                ])
                .unwrap();
        }

        let mut stmt = self.connection
            .prepare_cached(
                r#"
            INSERT OR IGNORE INTO episode (path, anime, episode, season, special)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            )
            .unwrap();
        let list = self.anime_directory.iter().flat_map(|v| {
            WalkDir::new(v)
                .max_depth(5)
                .min_depth(2)
                .into_iter()
                .filter_map(|d| Some(d.ok()?))
                .filter(|d| {
                    d.file_type().is_file()
                        && d.path()
                            .extension()
                            .map(|e| matches!(e.to_str().unwrap(), "mkv" | "mp4" | "ts"))
                            .unwrap_or(false)
                })
                .filter_map(|dir_entry| {
                    let episode = Episode::from_str(dir_entry.path().to_str()?).ok()?;
                    let mut anime_directory = dir_entry.path().parent()?;

                    // Walk to parent directory
                    for _ in 0..dir_entry.depth() - 2 {
                        anime_directory = anime_directory.parent()?;
                    }
                    Some((
                        dir_entry.path().to_str()?.to_owned(),
                        anime_directory.file_name()?.to_str()?.to_owned(),
                        episode,
                    ))
                })
        });
        for i in list {
            match i.2 {
                Episode::Numbered { season, episode, .. } => {
                    stmt.execute(params![i.0, i.1, episode, season, None::<String>])
                        .unwrap();
                }
                Episode::Special { filename, .. } => {
                    stmt.execute(params![i.0, i.1, None::<u32>, None::<u32>, filename.trim()])
                        .unwrap();
                }
            }
        }
    }

    pub fn update_watch(&mut self, directory: &str, watched: Episode) {
        unimplemented!()
    }

    pub fn episodes(&mut self, directory: &str) -> BTreeSet<Episode> {
        unimplemented!()
    }

    pub fn directories(&self) {
        unimplemented!()
    }

    pub fn next_episode(&self, directory: &str) -> Episode {
        unimplemented!()
    }

    pub fn current_episode(&self, directory: &str) -> Episode {
        unimplemented!()
    }
}
