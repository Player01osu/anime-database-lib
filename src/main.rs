use anime_database_lib::{
    database::Database,
    episode::Episode,
};

const DATABASE_PATH: &str = "./anime.db";
const ANIME_DIR: &str = "/home/bruh/Videos/not-anime";

fn main() {
    let mut db = Database::new(DATABASE_PATH, vec![ANIME_DIR]).unwrap();
    let anime = db.get_anime(r#"[Bulldog] Yuru Yuri S2 [BD 1080p HEVC FLAC]"#).unwrap();
    dbg!(anime.update_watched(Episode::from((1, 5)))).ok();
    dbg!(anime);
    dbg!(db.animes().unwrap().into_iter().map(|(v, _)| v.to_owned()).collect::<Vec<String>>());
    db.write(DATABASE_PATH).ok();
}
