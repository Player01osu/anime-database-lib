use anime_database_lib::{
    database::{Database, EpisodeMap},
    episode::Episode,
};

const DATABASE_PATH: &str = "./anime.db";
const ANIME_DIR: &str = "/home/bruh/Videos/not-anime";

fn update_episode(db: &Database, anime: &str, episodes: &EpisodeMap, new_episode: Episode) {
    println!();
    println!(r#"Updating "{anime}" to {new_episode}"#);
    db.update_watched(anime, new_episode, &episodes).unwrap();
    println!();
}

fn main() {
    let db = Database::new(DATABASE_PATH, vec![ANIME_DIR]).unwrap();
    db.init_db().unwrap();
    db.update().unwrap();
    let animes = dbg!(db.animes()).unwrap();
    dbg!(db.episodes(&animes[0])).unwrap();
    dbg!(db.episodes(&animes[1])).unwrap();
    dbg!(db.episodes(&animes[2])).unwrap();
    dbg!(db.episodes(&animes[3])).unwrap();
    dbg!(db.episodes(&animes[4])).unwrap();

    let anime = &animes[0];
    let episodes = db.episodes(&animes[0]).unwrap();

    update_episode(&db, anime, &episodes, Episode::from((1, 1)));

    let current_episode = db.current_episode(anime).unwrap();
    let next_episode = db.next_episode_raw(current_episode, &episodes).unwrap();
    let episode_path = &episodes[&Episode::from(current_episode)][0];

    println!(
        r#"{anime}: Currently on S{:02} E{:02} at path:
"{episode_path}"

Next is: {next_episode}"#,
        current_episode.0, current_episode.1
    );

    update_episode(&db, anime, &episodes, Episode::from((1, 2)));

    let current_episode = db.current_episode(anime).unwrap();
    let next_episode = db.next_episode(anime, &episodes).unwrap().unwrap();
    let episode_path = &episodes[&Episode::from(current_episode)][0];

    println!(
        r#"{anime}: Currently on S{:02} E{:02} at path:
"{episode_path}"

Next is: {next_episode}"#,
        current_episode.0, current_episode.1
    );
}
