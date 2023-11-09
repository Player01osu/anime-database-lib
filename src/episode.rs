use std::{
    path::Path,
    str::FromStr,
};

use regex::Regex;
lazy_static::lazy_static! {
    static ref REG_EPS: Regex = Regex::new(r#"(?:(?:^|S|s)(?P<s>\d{2}))?(?:_|x|E|e|EP|ep| )(?P<e>\d{1,2})(?:.bits|_| |-|\.|v|$)"#).unwrap();
    static ref REG_PARSE_OUT: Regex = Regex::new(r#"(x256|x265|\d{4}|\d{3})|10.bits"#).unwrap();
    static ref REG_SPECIAL: Regex =
    Regex::new(r#".*OVA.*\.|NCED.*? |NCOP.*? |(-|_| )(ED|OP|SP|no-credit_opening|no-credit_ending).*?(-|_| )"#).unwrap();
}

#[derive(Debug, PartialEq)]
pub enum Episode {
    Numbered {
        season: usize,
        episode: usize,
        filepath: String,
    },
    Special {
        filename: String,
        filepath: String,
    },
}

impl PartialOrd for Episode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match self {
            Self::Numbered {
                season: season_a,
                episode: episode_a,
                ..
            } => match other {
                Self::Numbered {
                    season: season_b,
                    episode: episode_b,
                    ..
                } => {
                    if season_a == season_b {
                        Some(episode_a.cmp(episode_b))
                    } else {
                        Some(season_a.cmp(season_b))
                    }
                }
                Self::Special { .. } => Some(std::cmp::Ordering::Greater),
            },
            Self::Special {
                filename: filename_a,
                ..
            } => match other {
                Self::Numbered { .. } => Some(std::cmp::Ordering::Less),
                Self::Special {
                    filename: filename_b,
                    ..
                } => Some(filename_a.cmp(filename_b)),
            },
        }
    }
}

impl FromStr for Episode {
    type Err = ();
    fn from_str(path: &str) -> Result<Self, Self::Err> {
        let filename = || {
            Path::new(path)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
        };
        if REG_SPECIAL.is_match(path) {
            return Ok(Self::Special {
                filepath: path.to_string(),
                filename: filename(),
            });
        }

        match REG_EPS.captures(&REG_PARSE_OUT.replace_all(path, "#")) {
            Some(caps) => {
                let season = caps
                    .name("s")
                    .map(|a| a.as_str().parse().expect("Capture is integer"))
                    .unwrap_or(1);
                let episode = caps
                    .name("e")
                    .map(|a| a.as_str().parse().expect("Capture is integer"))
                    .unwrap_or(1);
                return Ok(Self::Numbered {
                    season,
                    episode,
                    filepath: path.to_string(),
                });
            }
            None => {
                return Ok(Self::Special {
                    filepath: path.to_string(),
                    filename: filename(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn episode_sort_0() {
        let a = Episode::Numbered {
            season: 1,
            episode: 1,
            filepath: String::from("abc"),
        };
        let b = Episode::Numbered {
            season: 1,
            episode: 2,
            filepath: String::from("abc"),
        };
        assert!(a < b);
    }

    #[test]
    fn episode_sort_1() {
        let a = Episode::Numbered {
            season: 1,
            episode: 2,
            filepath: String::from("abc"),
        };
        let b = Episode::Numbered {
            season: 2,
            episode: 1,
            filepath: String::from("abc"),
        };
        assert!(a < b);
    }

    #[test]
    fn episode_sort_2() {
        let a = Episode::Special {
            filepath: String::from("abc"),
            filename: String::from("abc"),
        };
        let b = Episode::Numbered {
            season: 2,
            episode: 1,
            filepath: String::from("abc"),
        };
        assert!(a < b);
    }

    #[test]
    fn episode_sort_3() {
        let a = Episode::Numbered {
            season: 2,
            episode: 1,
            filepath: String::from("abc"),
        };
        let b = Episode::Special {
            filepath: String::from("abc"),
            filename: String::from("abc"),
        };
        assert!(a > b);
    }

    #[test]
    fn episode_from_str_0() {
        let filename = r"[sam] Vinland Saga - 24 [BD 1080p FLAC] [6696F95B].mkv".to_string();
        assert_eq!(
            Ok(Episode::Numbered {
                season: 1,
                episode: 24,
                filepath: filename.clone()
            }),
            Episode::from_str(&filename)
        );
    }

    #[test]
    fn episode_from_str_1() {
        let filename =
            r"Girls.und.Panzer.S01E04.1080p-Hi10p.BluRay.FLAC2.1.x264-CTR.[1123C40D].mkv"
                .to_string();
        assert_eq!(
            Ok(Episode::Numbered {
                season: 1,
                episode: 4,
                filepath: filename.clone()
            }),
            Episode::from_str(&filename)
        );
    }

    #[test]
    fn episode_from_str_2() {
        let filename = r"[Datte13] Yuyushiki - S01E12 - Uneventful Good Life.mkv".to_string();
        assert_eq!(
            Ok(Episode::Numbered {
                season: 1,
                episode: 12,
                filepath: filename.clone()
            }),
            Episode::from_str(&filename)
        );
    }

    #[test]
    fn episode_from_str_3() {
        let filename = r"[Arid] Sound! Euphonium - Creditless OP [D04F5D1D].mkv".to_string();
        assert_eq!(
            Ok(Episode::Special {
                filepath: filename.clone(),
                filename: filename.clone()
            }),
            Episode::from_str(&filename)
        );
    }
}
