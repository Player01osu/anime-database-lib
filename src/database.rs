use rusqlite::{params, Connection};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap},
    fs,
    str::FromStr,
    thread::{self, JoinHandle},
};
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
        let sqlite_conn = Connection::open(&path)
            .map_err(|e| eprintln!("Failed to connect to sqlite database: {e}"))
            .unwrap();
        Self {
            path,
            anime_directory,
            connection: sqlite_conn,
        }
    }

    pub fn write(&mut self) {
        unimplemented!()
    }

    pub fn thread_update(&mut self) -> JoinHandle<()> {
        let path_move = self.path.clone();
        let anime_directory_move = self.anime_directory.clone();

        thread::spawn(move || {
            let path = path_move;
            let anime_directory = anime_directory_move;
            let sqlite_conn = Connection::open(&path)
                .map_err(|e| eprintln!("Failed to connect to sqlite database: {e}"))
                .unwrap();
            sqlite_conn.execute_batch(IMPORTS).unwrap();
            let mut database_async = Self {
                path,
                anime_directory,
                connection: sqlite_conn,
            };
            database_async.update()
        })
    }

    pub fn update(&mut self) {
        let mut stmt_anime = self
            .connection
            .prepare_cached(
                r#"
            INSERT OR IGNORE INTO anime (anime)
            VALUES (?1)
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
        }

        let mut stmt = self
            .connection
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
                Episode::Numbered {
                    season, episode, ..
                } => {
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

    pub fn update_watched(&mut self, directory: &str, watched: Episode) {
        let (season, episode) = match watched {
            Episode::Numbered {
                season, episode, ..
            } => (season, episode),
            Episode::Special { .. } => {
                eprintln!("Special episode watched not implemented");
                return;
            }
        };

        let query = r#"
            UPDATE anime
            SET season='?1', episode='?2'
            WHERE anime='?3';
        "#;

        self.connection
            .execute(query, params![season, episode, directory])
            .unwrap();
    }

    pub fn episodes(&mut self, directory: &str) -> BTreeMap<Episode, Box<[String]>> {
        let mut episode_stmt = self
            .connection
            .prepare_cached(
                r#"
            SELECT (path, episode, season, special)
            FROM episode
            WHERE anime='?1'
            "#,
            )
            .unwrap();

        // TODO: Double query into same table.
        let mut filepath_stmt = self
            .connection
            .prepare_cached(
                r#"
            SELECT (path)
            FROM episode
            WHERE season='?1' AND episode='?2';
            "#,
            )
            .unwrap();

        episode_stmt
            .query_map(params![directory], |rows| {
                match rows.get_unwrap(3) {
                    // Special
                    Some(filename) => Ok((
                        Episode::Special { filename },
                        vec![rows.get_unwrap(0)].into(),
                    )),
                    None => {
                        let episode = rows.get_unwrap(1);
                        let season = rows.get_unwrap(2);
                        let filepaths = filepath_stmt
                            .query_map(params![season, episode], |rows| Ok(rows.get_unwrap(0)))
                            .unwrap()
                            .filter_map(|rows| rows.ok())
                            .collect::<Box<[String]>>();
                        Ok((Episode::Numbered { season, episode }, filepaths))
                    }
                }
            })
            .unwrap()
            .filter_map(|rows| rows.ok())
            .collect::<BTreeMap<Episode, Box<[String]>>>()
    }

    pub fn directories(&self) -> Vec<String> {
        let mut anime_stmt = self
            .connection
            .prepare_cached(
                r#"
        SELECT (last_watched, anime)
        FROM anime;
        "#,
            )
            .unwrap();

        let mut heap = anime_stmt
            .query_map([], |rows| {
                Ok(Reverse((rows.get_unwrap(0), rows.get_unwrap(1))))
            })
            .unwrap()
            .filter_map(|rows| rows.ok())
            .collect::<BinaryHeap<Reverse<(Option<usize>, String)>>>();

        // TODO: use into_iter_sorted when it gets stabilized.
        //
        // https://github.com/rust-lang/rust/issues/59278
        let mut vec = vec![];
        while let Some(Reverse((_, anime))) = heap.pop() {
            vec.push(anime);
        }
        vec
    }

    pub fn next_episode<'a>(
        &self,
        directory: &str,
        episodes: &'a BTreeMap<Episode, Box<[String]>>,
    ) -> Option<&'a Episode> {
        let (season, episode) = self.current_episode(directory);

        let get_episode = |season, episode| {
            episodes
                .get_key_value(&Episode::Numbered { season, episode })
                .map(|v| v.0)
        };

        if let Some(episode) = get_episode(season, episode + 1) {
            Some(episode)
        } else if let Some(episode) = get_episode(season + 1, 0) {
            Some(episode)
        } else if let Some(episode) = get_episode(season + 1, 1) {
            Some(episode)
        } else {
            None
        }
    }

    /// Gets current episode of directory in (season, episode) form.
    pub fn current_episode(&self, directory: &str) -> (usize, usize) {
        let query = r#"
        SELECT (current_season, current_episode)
        FROM anime
        WHERE anime='?1'
        "#;
        self.connection
            .query_row(query, [directory], |rows| {
                Ok((rows.get_unwrap(0), rows.get_unwrap(1)))
            })
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::{cmp::Reverse, collections::BinaryHeap};

    #[test]
    fn heap_test() {
        let mut heap = BinaryHeap::new();
        heap.push(Reverse((400, "a")));
        heap.push(Reverse((400, "b")));
        heap.push(Reverse((400, "c")));
        heap.push(Reverse((10, "d")));
        heap.push(Reverse((300, "e")));

        assert_eq!(
            heap.into_sorted_vec().as_slice(),
            [
                Reverse((400, "c")),
                Reverse((400, "b")),
                Reverse((400, "a")),
                Reverse((300, "e")),
                Reverse((10, "d"))
            ]
        );
    }
}
