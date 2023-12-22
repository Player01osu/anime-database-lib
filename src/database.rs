use crate::episode::Episode;
use flexbuffers::DeserializationError;
use std::collections::btree_map::Entry;
use std::fs::{metadata, read_dir, File};
use std::io::{Read, Write};
use std::{collections::BTreeMap, path::Path, time::SystemTime};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anime {
    path: String,
    last_watched: u64,
    last_updated: u64,
    current_episode: Episode,
    episodes: EpisodeMap,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Database {
    anime_map: BTreeMap<String, Anime>,
}

pub type EpisodeMap = Vec<(Episode, Vec<String>)>;

#[derive(Debug, Error)]
pub enum InvalidEpisodeError {
    #[error("{episode} Does not exist in \"{anime}\"")]
    NotExist { anime: String, episode: Episode },
}

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("{0}")]
    IO(std::io::Error),
    #[error("{0}")]
    Deserialization(DeserializationError),
    #[error("Invalid path to episode")]
    InvalidFile,
    #[error("Unable to convert file to UTF-8 string")]
    UTF8,
    #[error("{0}")]
    InvalidEpisode(InvalidEpisodeError),
}

type Err = DatabaseError;

impl From<std::io::Error> for Err {
    fn from(v: std::io::Error) -> Self {
        Self::IO(v)
    }
}

impl From<DeserializationError> for Err {
    fn from(v: DeserializationError) -> Self {
        Self::Deserialization(v)
    }
}

type Result<T> = std::result::Result<T, Err>;

macro_rules! o_to_str {
    ($x: expr) => {
        $x.to_str().unwrap().to_string()
    };
}

fn get_time() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

impl Anime {
    pub fn from_path(path: impl AsRef<Path>, time: u64) -> Self {
        let path = path.as_ref();
        let mut anime = Anime {
            path: o_to_str!(path),
            last_watched: 0,
            last_updated: time,
            current_episode: Episode::from((1, 1)),
            episodes: Vec::new(),
        };
        anime.update_episodes();
        anime
    }

    pub fn update_episodes(&mut self) {
        WalkDir::new(&self.path)
            .max_depth(5)
            .min_depth(1)
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
                let episode = Episode::try_from(dir_entry.path()).ok()?;
                let path = dir_entry.path().to_str()?.to_owned();

                Some((episode, path))
            })
            .for_each(
                |(ep, path)| match self.episodes.iter_mut().find(|(v, _)| ep.eq(v)) {
                    Some((_, paths)) => paths.push(path.clone()),
                    None => self.episodes.push((ep, vec![path])),
                },
            );
        self.episodes.sort_by(|(a, _), (b, _)| a.cmp(b));
    }

    /// Gets current episode of directory in (season, episode) form.
    pub fn current_episode(&self) -> Episode {
        self.current_episode.clone()
    }

    pub fn next_episode<'a>(&self) -> Result<Option<Episode>> {
        match self.current_episode {
            Episode::Numbered { season, episode } => Ok(self.next_episode_raw((season, episode))),
            Episode::Special { .. } => Ok(None),
        }
    }

    pub fn next_episode_raw<'a>(
        &self,
        _current_episode @ (season, episode): (u32, u32),
    ) -> Option<Episode> {
        let get_episode = |season, episode| {
            self.episodes
                .iter()
                .find(|(ep, _)| ep.eq(&Episode::Numbered { season, episode }))
                .map(|v| v.0.clone())
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

    pub fn episodes(&self) -> &EpisodeMap {
        &self.episodes
    }

    /// Prefer `.update_watched` because it checks if episode exists in episode_map.
    pub unsafe fn update_watched_unchecked(&mut self, watched: Episode) {
        let timestamp = get_time();
        self.last_watched = timestamp;
        self.current_episode = watched;
    }

    pub fn update_watched(&mut self, watched: Episode) -> Result<()> {
        match self.episodes.iter().find(|(ep, _)| watched.eq(ep)) {
            Some(_) => Ok(unsafe { self.update_watched_unchecked(watched) }),
            None => Err(Err::InvalidEpisode(InvalidEpisodeError::NotExist {
                anime: self.path.to_string(),
                episode: watched,
            })),
        }
    }
}

fn dir_modified_time(path: impl AsRef<Path>) -> u64 {
    metadata(path)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

impl Database {
    /// Note: If database has not been created, then `.init_db()`
    /// must be run before using.
    pub fn new(path: impl AsRef<str>, anime_directories: Vec<impl AsRef<str>>) -> Result<Self> {
        let path = path.as_ref();
        match File::open(path) {
            Ok(mut v) => {
                let mut slice = vec![];
                v.read_to_end(&mut slice)?;
                Ok(flexbuffers::from_slice::<Self>(&slice)?)
            }
            Err(_) => {
                let mut db = Self {
                    anime_map: BTreeMap::new(),
                };
                db.update(anime_directories);
                Ok(db)
            }
        }
    }

    pub fn update(&mut self, anime_directories: Vec<impl AsRef<str>>) {
        let time = get_time();
        anime_directories
            .iter()
            .filter_map(|s| read_dir(s.as_ref()).ok())
            .flat_map(|s| {
                s.filter_map(|v| v.ok())
                    .map(|v| (o_to_str!(v.file_name()), v.path()))
            })
            .for_each(|(name, path)| {
                match self.anime_map.entry(name) {
                    Entry::Vacant(v) => {
                        v.insert(Anime::from_path(path, time));
                    }
                    Entry::Occupied(mut v) => {
                        if v.get().last_updated < dir_modified_time(path) {
                            v.get_mut().update_episodes();
                        }
                    }
                };
            });
    }

    pub fn write(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let mut f = File::create(path)?;
        let mut s = flexbuffers::FlexbufferSerializer::new();
        self.serialize(&mut s).unwrap();
        f.write_all(s.view())?;
        Ok(())
    }

    pub fn animes(&mut self) -> Result<Box<[(&String, &mut Anime)]>> {
        let mut anime_list = self
            .anime_map
            .iter_mut()
            .collect::<Box<[(&String, &mut Anime)]>>();
        anime_list.sort_by(|(_, a), (_, b)| b.last_watched.cmp(&a.last_watched));

        Ok(anime_list)
    }

    pub fn get_anime<'a>(&'a mut self, anime: impl AsRef<str>) -> Option<&'a mut Anime> {
        let anime = anime.as_ref().to_string();
        self.anime_map.get_mut(&anime)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

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
