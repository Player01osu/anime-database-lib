use rusqlite::{params, Connection};
use std::{
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

// Comparison wrapper for time and title.
//
// Sort by recently watched, otherwise, sort by title.
#[derive(PartialEq, Eq, Ord, Debug)]
struct TimeTitle((Option<usize>, String));

impl PartialOrd for TimeTitle {
    fn partial_cmp(&self, TimeTitle((time_b, title_b)): &Self) -> Option<std::cmp::Ordering> {
        let TimeTitle((time_a, title_a)) = self;
        if time_a.eq(time_b) {
            Some(title_a.cmp(&title_b))
        } else {
            Some(time_b.cmp(&time_a))
        }
    }
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

    pub fn update_watched(&mut self, anime: &str, watched: Episode) {
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
            .execute(query, params![season, episode, anime])
            .unwrap();
    }

    pub fn episodes(&mut self, anime: &str) -> BTreeMap<Episode, Vec<String>> {
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

        episode_stmt
            .query_map(params![anime], |rows| {
                let filepath: String = rows.get_unwrap(0);
                match rows.get_unwrap(3) {
                    // Special
                    Some(filename) => Ok((Episode::Special { filename }, filepath)),
                    None => {
                        let episode = rows.get_unwrap(1);
                        let season = rows.get_unwrap(2);
                        Ok((Episode::Numbered { season, episode }, filepath))
                    }
                }
            })
            .unwrap()
            .filter_map(|rows| rows.ok())
            .fold(BTreeMap::new(), |mut acc, (k, v)| {
                // TODO: Remove clone
                acc.entry(k)
                    .and_modify(|list| list.push(v.clone()))
                    .or_insert(vec![v]);
                acc
            })
    }

    pub fn directories(&self) -> Box<[String]> {
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
                Ok(TimeTitle((rows.get_unwrap(0), rows.get_unwrap(1))))
            })
            .unwrap()
            .filter_map(|rows| rows.ok())
            .collect::<BinaryHeap<TimeTitle>>();

        // TODO: use into_iter_sorted when it gets stabilized.
        //
        // https://github.com/rust-lang/rust/issues/59278
        let mut vec = vec![];
        while let Some(TimeTitle((_, anime))) = heap.pop() {
            vec.push(anime);
        }
        vec.into()
    }

    pub fn next_episode<'a>(
        &self,
        anime: &str,
        episodes: &'a BTreeMap<Episode, Box<[String]>>,
    ) -> Option<&'a Episode> {
        let current_episode = self.current_episode(anime);
        self.next_episode_from_current(current_episode, episodes)
    }

    pub fn next_episode_from_current<'a>(
        &self,
        _current_episode @ (season, episode): (usize, usize),
        episodes: &'a BTreeMap<Episode, Box<[String]>>,
    ) -> Option<&'a Episode> {
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
    pub fn current_episode(&self, anime: &str) -> (usize, usize) {
        let query = r#"
        SELECT (current_season, current_episode)
        FROM anime
        WHERE anime='?1'
        "#;
        self.connection
            .query_row(query, [anime], |rows| {
                Ok((rows.get_unwrap(0), rows.get_unwrap(1)))
            })
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BinaryHeap};

    use crate::database::TimeTitle;

    #[test]
    fn heap_test() {
        let mut heap = BinaryHeap::new();
        heap.push(TimeTitle((None, "a".to_string())));
        heap.push(TimeTitle((Some(300), "e".to_string())));
        heap.push(TimeTitle((None, "c".to_string())));
        heap.push(TimeTitle((Some(400), "c".to_string())));
        heap.push(TimeTitle((Some(400), "b".to_string())));
        heap.push(TimeTitle((None, "b".to_string())));
        heap.push(TimeTitle((Some(10), "d".to_string())));
        heap.push(TimeTitle((Some(400), "a".to_string())));

        assert_eq!(
            heap.into_sorted_vec().as_slice(),
            [
                TimeTitle((Some(400), "a".to_string())),
                TimeTitle((Some(400), "b".to_string())),
                TimeTitle((Some(400), "c".to_string())),
                TimeTitle((Some(300), "e".to_string())),
                TimeTitle((Some(10), "d".to_string())),
                TimeTitle((None, "a".to_string())),
                TimeTitle((None, "b".to_string())),
                TimeTitle((None, "c".to_string())),
            ]
        );
    }

    #[test]
    fn btree_test() {
        let btree = [("hello", 20), ("hi", 5), ("hello", 1)].into_iter().fold(
            BTreeMap::new(),
            |mut acc, (k, v)| {
                acc.entry(k)
                    .and_modify(|list: &mut Vec<usize>| list.push(v))
                    .or_insert(vec![v]);
                acc
            },
        );
        assert_eq!(
            BTreeMap::from([("hello", vec![20, 1]), ("hi", vec![5])]),
            btree
        );
    }
}
