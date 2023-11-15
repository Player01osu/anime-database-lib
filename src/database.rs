use crate::{episode::Episode, imports::IMPORTS};
use std::{
    collections::{BTreeMap, BinaryHeap},
    fs,
    str::FromStr,
    thread::{self, JoinHandle},
};

use rusqlite::{params, Connection};
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Debug)]
pub struct Database {
    path: String,
    anime_directory: Vec<String>,
    connection: Connection,
}

pub type EpisodeMap = BTreeMap<Episode, Vec<String>>;

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

#[derive(Debug, Error)]
pub enum InvalidEpisodeError {
    #[error("{episode} Does not exist in \"{anime}\"")]
    NotExist { anime: String, episode: Episode },
}

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("{0}")]
    Rusqlite(rusqlite::Error),
    #[error("{0}")]
    IO(std::io::Error),
    #[error("Invalid path to episode")]
    InvalidFile,
    #[error("Unable to convert file to UTF-8 string")]
    UTF8,
    #[error("{0}")]
    InvalidEpisode(InvalidEpisodeError),
}

type Err = DatabaseError;

impl From<rusqlite::Error> for Err {
    fn from(v: rusqlite::Error) -> Self {
        Self::Rusqlite(v)
    }
}

impl From<std::io::Error> for Err {
    fn from(v: std::io::Error) -> Self {
        Self::IO(v)
    }
}

type Result<T> = std::result::Result<T, Err>;

impl Database {
    /// Note: If database has not been created, then `.init_db()`
    /// must be run before using.
    pub fn new(path: impl AsRef<str>, anime_directory: Vec<impl AsRef<str>>) -> Result<Self> {
        let path = path.as_ref().to_string();
        let anime_directory = anime_directory
            .iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        let sqlite_conn = Connection::open(&path)?;
        Ok(Self {
            path,
            anime_directory,
            connection: sqlite_conn,
        })
    }

    pub fn init_db(&self) -> Result<()> {
        self.connection.execute_batch(IMPORTS)?;
        Ok(())
    }

    pub fn threaded_update(&self) -> JoinHandle<Result<()>> {
        let path_move = self.path.clone();
        let anime_directory_move = self.anime_directory.clone();

        thread::spawn(move || {
            let path = path_move;
            let anime_directory = anime_directory_move;
            let sqlite_conn = Connection::open(&path)?;
            let database_async = Self {
                path,
                anime_directory,
                connection: sqlite_conn,
            };
            database_async.update()
        })
    }

    pub fn update(&self) -> Result<()> {
        let mut stmt_anime = self.connection.prepare_cached(
            r#"
            INSERT OR IGNORE INTO anime (name)
            VALUES (?1)
        "#,
        )?;

        let list = self
            .anime_directory
            .iter()
            .filter_map(|v| fs::read_dir(v).ok())
            .flat_map(|v| v.map(|d| d.map(|v| v.path())));
        for i in list {
            stmt_anime.execute(params![i?
                .file_name()
                .ok_or(Err::InvalidFile)?
                .to_string_lossy(),])?;
        }

        let mut stmt = self.connection.prepare_cached(
            r#"
            INSERT OR IGNORE INTO episode (path, anime, episode, season, special)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
        )?;
        let list = self.anime_directory.iter().flat_map(|v| {
            WalkDir::new(v)
                .max_depth(5)
                .min_depth(2)
                .into_iter()
                .filter_map(|d| Some(d.ok()?)) // Report directory not found
                .filter(|d| {
                    d.file_type().is_file()
                        && d.path()
                            .extension()
                            .map(|e| matches!(e.to_str(), Some("mkv") | Some("mp4") | Some("ts")))
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
                    stmt.execute(params![i.0, i.1, episode, season, None::<String>])?;
                }
                Episode::Special { filename, .. } => {
                    stmt.execute(params![i.0, i.1, None::<u32>, None::<u32>, filename.trim()])?;
                }
            }
        }
        Ok(())
    }

    /// Prefer `.update_watched` because it checks if episode exists in episode_map.
    pub unsafe fn update_watched_unchecked(&self, anime: &str, watched: Episode) -> Result<usize> {
        let (season, episode) = match watched {
            Episode::Numbered {
                season, episode, ..
            } => (season, episode),
            Episode::Special { .. } => {
                eprintln!("Special episode watched not implemented");
                return Ok(0);
            }
        };

        let query = r#"
            UPDATE anime
            SET current_season=?1, current_episode=?2
            WHERE name=?3;
        "#;

        Ok(self
            .connection
            .execute(query, params![season, episode, anime])?)
    }

    pub fn update_watched(
        &self,
        anime: &str,
        watched: Episode,
        episodes: &EpisodeMap,
    ) -> Result<usize> {
        match episodes.get(&watched) {
            Some(_) => unsafe { self.update_watched_unchecked(anime, watched) },
            None => Err(Err::InvalidEpisode(InvalidEpisodeError::NotExist {
                anime: anime.to_string(),
                episode: watched,
            })),
        }
    }

    pub fn episodes(&self, anime: &str) -> Result<EpisodeMap> {
        let mut episode_stmt = self.connection.prepare_cached(
            r#"
            SELECT path, episode, season, special
            FROM episode
            WHERE anime=?1
            "#,
        )?;

        let episode_map = episode_stmt
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
            })?
            .filter_map(|rows| rows.ok())
            .fold(BTreeMap::new(), |mut acc, (k, v)| {
                // TODO: Remove clone
                acc.entry(k)
                    .and_modify(|list: &mut Vec<String>| list.push(v.clone()))
                    .or_insert(vec![v]);
                acc
            });
        Ok(episode_map)
    }

    pub fn animes(&self) -> Result<Box<[String]>> {
        let mut anime_stmt = self.connection.prepare_cached(
            r#"
        SELECT last_watched, name
        FROM anime;
        "#,
        )?;

        let mut heap = anime_stmt
            .query_map([], |rows| {
                Ok(TimeTitle((rows.get_unwrap(0), rows.get_unwrap(1))))
            })?
            .filter_map(|rows| rows.ok())
            .collect::<BinaryHeap<TimeTitle>>();

        // TODO: use into_iter_sorted when it gets stabilized.
        //
        // https://github.com/rust-lang/rust/issues/59278
        let mut vec = vec![];
        while let Some(TimeTitle((_, anime))) = heap.pop() {
            vec.push(anime);
        }
        Ok(vec.into())
    }

    pub fn next_episode<'a>(
        &self,
        anime: &str,
        episodes: &'a EpisodeMap,
    ) -> Result<Option<&'a Episode>> {
        let current_episode = self.current_episode(anime)?;
        Ok(self.next_episode_raw(current_episode, episodes))
    }

    pub fn next_episode_raw<'a>(
        &self,
        _current_episode @ (season, episode): (usize, usize),
        episodes: &'a EpisodeMap,
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
    pub fn current_episode(&self, anime: &str) -> Result<(usize, usize)> {
        let query = r#"
        SELECT current_season, current_episode
        FROM anime
        WHERE name=?1
        "#;
        Ok(self.connection.query_row(query, [anime], |rows| {
            Ok((rows.get(0).unwrap_or(1), rows.get(1).unwrap_or(1)))
        })?)
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
