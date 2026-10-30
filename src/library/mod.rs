use crate::model::{Playlist, Settings, Track};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct Library {
    conn: Arc<parking_lot::Mutex<Connection>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    pub tracks: Vec<Track>,
    pub current: Option<usize>,
    pub position: f64,
}

const COLUMNS: &str = "t.id,t.source,t.source_id,t.title,t.artist,t.album,t.duration_secs,t.artwork_url,t.play_url,t.meta";

#[derive(Default, Serialize, Deserialize)]
struct Meta {
    #[serde(default)]
    artist_refs: Vec<crate::model::ArtistRef>,
    #[serde(default)]
    album_id: Option<String>,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn upsert(conn: &Connection, tracks: &[Track]) -> anyhow::Result<()> {
    let mut stmt = conn.prepare_cached("INSERT INTO tracks VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
        ON CONFLICT(id) DO UPDATE SET source=excluded.source,source_id=excluded.source_id,
        title=excluded.title,artist=excluded.artist,album=excluded.album,
        duration_secs=excluded.duration_secs,artwork_url=excluded.artwork_url,play_url=excluded.play_url,
        meta=excluded.meta")?;
    for t in tracks {
        stmt.execute(params![
            t.id,
            serde_json::to_string(&t.source)?,
            t.source_id,
            t.title,
            t.artist,
            t.album,
            t.duration_secs,
            t.artwork_url,
            t.play_url,
            meta_json(&merged_meta(conn, t)?)?
        ])?;
    }
    Ok(())
}

/// Keeps what is already known about a track's links when an update only knows part of it.
fn merged_meta(conn: &Connection, t: &Track) -> anyhow::Result<Track> {
    if !t.artist_refs.is_empty() && t.album_id.is_some() {
        return Ok(t.clone());
    }
    let old: Option<String> = conn
        .query_row("SELECT meta FROM tracks WHERE id=?1", [&t.id], |r| r.get(0))
        .optional()?
        .flatten();
    let old: Meta = old
        .and_then(|m| serde_json::from_str(&m).ok())
        .unwrap_or_default();
    let mut merged = t.clone();
    if merged.artist_refs.is_empty() {
        merged.artist_refs = old.artist_refs;
    }
    if merged.album_id.is_none() {
        merged.album_id = old.album_id;
    }
    Ok(merged)
}

fn meta_json(t: &Track) -> anyhow::Result<Option<String>> {
    if t.artist_refs.is_empty() && t.album_id.is_none() {
        return Ok(None);
    }
    Ok(Some(serde_json::to_string(&Meta {
        artist_refs: t.artist_refs.clone(),
        album_id: t.album_id.clone(),
    })?))
}

fn row_track(row: &rusqlite::Row<'_>) -> rusqlite::Result<Track> {
    let source: String = row.get(1)?;
    let meta: Meta = row
        .get::<_, Option<String>>(9)?
        .and_then(|m| serde_json::from_str(&m).ok())
        .unwrap_or_default();
    Ok(Track {
        id: row.get(0)?,
        source: serde_json::from_str(&source).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
        })?,
        source_id: row.get(2)?,
        title: row.get(3)?,
        artist: row.get(4)?,
        album: row.get(5)?,
        duration_secs: row.get(6)?,
        artwork_url: row.get(7)?,
        play_url: row.get(8)?,
        artist_refs: meta.artist_refs,
        album_id: meta.album_id,
    })
}

fn tracks_query(
    conn: &Connection,
    suffix: &str,
    params: impl rusqlite::Params,
) -> anyhow::Result<Vec<Track>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM tracks t {suffix}"))?;
    Ok(stmt
        .query_map(params, row_track)?
        .collect::<rusqlite::Result<_>>()?)
}

impl Library {
    pub fn open_default() -> anyhow::Result<Self> {
        Self::open(&crate::model::paths::data_dir().join("library.db"))
    }

    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        anyhow::ensure!(
            version <= 2,
            "Library schema {version} is newer than this Tonal version"
        );
        if version == 0 {
            let tx = conn.transaction()?;
            tx.execute_batch("CREATE TABLE tracks (
                id TEXT PRIMARY KEY, source TEXT NOT NULL, source_id TEXT NOT NULL,
                title TEXT NOT NULL, artist TEXT NOT NULL, album TEXT, duration_secs REAL,
                artwork_url TEXT, play_url TEXT NOT NULL);
                CREATE TABLE liked (track_id TEXT PRIMARY KEY REFERENCES tracks(id), added_at INTEGER NOT NULL);
                CREATE TABLE playlists (id INTEGER PRIMARY KEY, name TEXT NOT NULL, origin TEXT, created_at INTEGER NOT NULL);
                CREATE TABLE playlist_tracks (playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
                    position INTEGER NOT NULL, track_id TEXT NOT NULL REFERENCES tracks(id), PRIMARY KEY(playlist_id,position));
                CREATE TABLE plays (track_id TEXT NOT NULL REFERENCES tracks(id), played_at INTEGER NOT NULL);
                CREATE INDEX plays_track ON plays(track_id);
                CREATE TABLE downloads (track_id TEXT PRIMARY KEY REFERENCES tracks(id), path TEXT NOT NULL);
                CREATE TABLE local_tracks (track_id TEXT PRIMARY KEY REFERENCES tracks(id));
                CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                PRAGMA user_version=1;")?;
            tx.commit()?;
        }
        if version < 2 {
            let tx = conn.transaction()?;
            tx.execute_batch("ALTER TABLE tracks ADD COLUMN meta TEXT; PRAGMA user_version=2;")?;
            tx.commit()?;
        }
        Ok(Self {
            conn: Arc::new(parking_lot::Mutex::new(conn)),
        })
    }

    pub fn upsert_tracks(&self, tracks: &[Track]) -> anyhow::Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        upsert(&tx, tracks)?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_liked(&self, track: &Track, liked: bool) -> anyhow::Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        if liked {
            upsert(&tx, std::slice::from_ref(track))?;
            tx.execute(
                "INSERT INTO liked VALUES (?1,?2) ON CONFLICT(track_id) DO NOTHING",
                params![track.id, now()],
            )?;
        } else {
            tx.execute("DELETE FROM liked WHERE track_id=?1", [&track.id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn is_liked(&self, track_id: &str) -> bool {
        self.conn
            .lock()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM liked WHERE track_id=?1)",
                [track_id],
                |r| r.get(0),
            )
            .unwrap_or(false)
    }

    pub fn liked(&self) -> anyhow::Result<Vec<Track>> {
        tracks_query(
            &self.conn.lock(),
            "JOIN liked l ON l.track_id=t.id ORDER BY l.added_at DESC,l.rowid DESC",
            [],
        )
    }

    pub fn playlists(&self) -> anyhow::Result<Vec<Playlist>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT p.id,p.name,p.origin,COUNT(pt.track_id) FROM playlists p
            LEFT JOIN playlist_tracks pt ON pt.playlist_id=p.id GROUP BY p.id ORDER BY p.created_at,p.id")?;
        Ok(stmt
            .query_map([], |r| {
                Ok(Playlist {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    origin: r.get(2)?,
                    track_count: r.get::<_, i64>(3)? as usize,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn create_playlist(&self, name: &str, origin: Option<&str>) -> anyhow::Result<i64> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO playlists(name,origin,created_at) VALUES (?1,?2,?3)",
            params![name, origin, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn rename_playlist(&self, id: i64, name: &str) -> anyhow::Result<()> {
        self.conn.lock().execute(
            "UPDATE playlists SET name=?2 WHERE id=?1",
            params![id, name],
        )?;
        Ok(())
    }

    pub fn delete_playlist(&self, id: i64) -> anyhow::Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM playlists WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn playlist_tracks(&self, id: i64) -> anyhow::Result<Vec<Track>> {
        tracks_query(
            &self.conn.lock(),
            "JOIN playlist_tracks p ON p.track_id=t.id WHERE p.playlist_id=?1 ORDER BY p.position",
            [id],
        )
    }

    pub fn add_to_playlist(&self, id: i64, tracks: &[Track]) -> anyhow::Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        upsert(&tx, tracks)?;
        let start: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position)+1,0) FROM playlist_tracks WHERE playlist_id=?1",
            [id],
            |r| r.get(0),
        )?;
        for (i, t) in tracks.iter().enumerate() {
            tx.execute(
                "INSERT INTO playlist_tracks VALUES (?1,?2,?3)",
                params![id, start + i as i64, t.id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn edit_playlist(&self, id: i64, edit: impl FnOnce(&mut Vec<String>)) -> anyhow::Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let mut ids = {
            let mut stmt = tx.prepare(
                "SELECT track_id FROM playlist_tracks WHERE playlist_id=?1 ORDER BY position",
            )?;
            stmt.query_map([id], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?
        };
        edit(&mut ids);
        tx.execute("DELETE FROM playlist_tracks WHERE playlist_id=?1", [id])?;
        for (i, track_id) in ids.iter().enumerate() {
            tx.execute(
                "INSERT INTO playlist_tracks VALUES (?1,?2,?3)",
                params![id, i as i64, track_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn remove_from_playlist(&self, id: i64, position: usize) -> anyhow::Result<()> {
        self.edit_playlist(id, |ids| {
            if position < ids.len() {
                ids.remove(position);
            }
        })
    }

    pub fn move_in_playlist(&self, id: i64, from: usize, to: usize) -> anyhow::Result<()> {
        self.edit_playlist(id, |ids| {
            if from < ids.len() && to < ids.len() && from != to {
                let item = ids.remove(from);
                ids.insert(to, item);
            }
        })
    }

    pub fn record_play(&self, track: &Track) -> anyhow::Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        upsert(&tx, std::slice::from_ref(track))?;
        tx.execute("INSERT INTO plays VALUES (?1,?2)", params![track.id, now()])?;
        tx.commit()?;
        Ok(())
    }

    pub fn recent(&self, limit: usize) -> anyhow::Result<Vec<Track>> {
        tracks_query(&self.conn.lock(), "JOIN (SELECT track_id,MAX(rowid) AS seq,MAX(played_at) AS last FROM plays GROUP BY track_id) p
            ON p.track_id=t.id ORDER BY p.last DESC,p.seq DESC LIMIT ?1", [limit as i64])
    }

    pub fn most_played(&self, limit: usize) -> anyhow::Result<Vec<Track>> {
        tracks_query(
            &self.conn.lock(),
            "JOIN (SELECT track_id,COUNT(*) AS n,MAX(rowid) AS seq FROM plays GROUP BY track_id) p
            ON p.track_id=t.id ORDER BY p.n DESC,p.seq DESC LIMIT ?1",
            [limit as i64],
        )
    }

    pub fn set_download(&self, track: &Track, path: &Path) -> anyhow::Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        upsert(&tx, std::slice::from_ref(track))?;
        tx.execute("INSERT INTO downloads VALUES (?1,?2) ON CONFLICT(track_id) DO UPDATE SET path=excluded.path",
            params![track.id, path.to_string_lossy()])?;
        tx.commit()?;
        Ok(())
    }

    pub fn download_path(&self, track_id: &str) -> Option<PathBuf> {
        self.conn
            .lock()
            .query_row(
                "SELECT path FROM downloads WHERE track_id=?1",
                [track_id],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
            .map(PathBuf::from)
    }

    pub fn downloads(&self) -> anyhow::Result<Vec<Track>> {
        tracks_query(
            &self.conn.lock(),
            "JOIN downloads d ON d.track_id=t.id ORDER BY d.rowid DESC",
            [],
        )
    }

    pub fn remove_download(&self, track_id: &str) -> anyhow::Result<()> {
        self.conn
            .lock()
            .execute("DELETE FROM downloads WHERE track_id=?1", [track_id])?;
        Ok(())
    }

    pub fn replace_local_tracks(&self, tracks: &[Track]) -> anyhow::Result<()> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        upsert(&tx, tracks)?;
        tx.execute("DELETE FROM local_tracks", [])?;
        for t in tracks {
            tx.execute("INSERT OR IGNORE INTO local_tracks VALUES (?1)", [&t.id])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn local_tracks(&self) -> anyhow::Result<Vec<Track>> {
        tracks_query(
            &self.conn.lock(),
            "JOIN local_tracks l ON l.track_id=t.id ORDER BY t.artist COLLATE NOCASE,t.title COLLATE NOCASE,t.id",
            [],
        )
    }

    fn load_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        let json: String = self
            .conn
            .lock()
            .query_row("SELECT value FROM kv WHERE key=?1", [key], |r| r.get(0))
            .ok()?;
        serde_json::from_str(&json).ok()
    }

    fn save_json(&self, key: &str, value: &impl Serialize) -> anyhow::Result<()> {
        self.conn.lock().execute(
            "INSERT INTO kv VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }

    pub fn load_settings(&self) -> Settings {
        self.load_json("settings").unwrap_or_default()
    }
    pub fn save_settings(&self, settings: &Settings) -> anyhow::Result<()> {
        self.save_json("settings", settings)
    }
    pub fn save_session(&self, session: &Session) -> anyhow::Result<()> {
        self.save_json("session", session)
    }
    pub fn load_session(&self) -> Option<Session> {
        self.load_json("session")
    }

    /// Every played track with its number of plays since `since` (ms since the epoch,
    /// None for all time), most played first.
    pub fn play_counts(&self, since: Option<i64>) -> anyhow::Result<Vec<(Track, u64)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS},p.n FROM tracks t JOIN (SELECT track_id,COUNT(*) AS n,MAX(rowid) AS seq
            FROM plays WHERE played_at>=?1 GROUP BY track_id) p ON p.track_id=t.id
            ORDER BY p.n DESC,p.seq DESC"
        ))?;
        Ok(stmt
            .query_map([since.unwrap_or(i64::MIN)], |row| {
                Ok((row_track(row)?, row.get::<_, i64>(10)?.max(0) as u64))
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// The most played artists, counting a song for its first credited artist.
    pub fn top_artists(&self, since: Option<i64>, limit: usize) -> anyhow::Result<Vec<Top>> {
        Ok(top(self.play_counts(since)?, limit, |t| {
            let artist = t.artists().into_iter().next()?.name;
            (!artist.trim().is_empty()).then(|| (artist, String::new()))
        }))
    }

    /// The most played albums, grouped by album title and album artist.
    pub fn top_albums(&self, since: Option<i64>, limit: usize) -> anyhow::Result<Vec<Top>> {
        Ok(top(self.play_counts(since)?, limit, |t| {
            t.album_ref().map(|a| (a.title, a.artist))
        }))
    }
}

/// One artist or album in the listening stats.
#[derive(Clone, Debug, PartialEq)]
pub struct Top {
    pub title: String,
    /// The album artist; empty for artists.
    pub artist: String,
    pub plays: u64,
    /// Its most played song, for artwork and links.
    pub track: Track,
}

fn top(
    counts: Vec<(Track, u64)>,
    limit: usize,
    key: impl Fn(&Track) -> Option<(String, String)>,
) -> Vec<Top> {
    let mut list: Vec<Top> = Vec::new();
    for (track, plays) in counts {
        let Some((title, artist)) = key(&track) else {
            continue;
        };
        match list.iter_mut().find(|e| {
            e.title.trim().eq_ignore_ascii_case(title.trim())
                && e.artist.trim().eq_ignore_ascii_case(artist.trim())
        }) {
            Some(entry) => {
                entry.plays += plays;
                if entry.track.artwork_url.is_none() {
                    entry.track.artwork_url = track.artwork_url;
                }
            }
            None => list.push(Top {
                title,
                artist,
                plays,
                track,
            }),
        }
    }
    list.sort_by_key(|e| std::cmp::Reverse(e.plays));
    list.truncate(limit);
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SourceKind;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Db {
        library: Library,
        path: PathBuf,
    }
    impl Db {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "tonal-test-{}-{}-{}.db",
                std::process::id(),
                now(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            Self {
                library: Library::open(&path).unwrap(),
                path,
            }
        }
    }
    impl Drop for Db {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            for suffix in ["-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", self.path.display()));
            }
        }
    }
    fn track(id: &str) -> Track {
        Track {
            id: Track::make_id(SourceKind::YouTube, id),
            source: SourceKind::YouTube,
            source_id: id.into(),
            title: id.into(),
            artist: "artist".into(),
            album: Some("album".into()),
            duration_secs: Some(120.0),
            artwork_url: None,
            play_url: format!("https://youtu.be/{id}"),
            ..Default::default()
        }
    }

    #[test]
    fn playlists_and_contiguous_positions() -> anyhow::Result<()> {
        let db = Db::new();
        let l = &db.library;
        let id = l.create_playlist("first", Some("spotify"))?;
        let ts = vec![track("a"), track("b"), track("c"), track("a")];
        l.add_to_playlist(id, &ts)?;
        assert_eq!(l.playlists()?[0].track_count, 4);
        l.remove_from_playlist(id, 1)?;
        l.move_in_playlist(id, 2, 0)?;
        assert_eq!(
            l.playlist_tracks(id)?,
            vec![ts[0].clone(), ts[0].clone(), ts[2].clone()]
        );
        l.move_in_playlist(id, 0, 2)?;
        assert_eq!(
            l.playlist_tracks(id)?,
            vec![ts[0].clone(), ts[2].clone(), ts[0].clone()]
        );
        let positions: Vec<i64> = l
            .conn
            .lock()
            .prepare("SELECT position FROM playlist_tracks ORDER BY position")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        assert_eq!(positions, vec![0, 1, 2]);
        l.rename_playlist(id, "renamed")?;
        assert_eq!(l.playlists()?[0].name, "renamed");
        l.delete_playlist(id)?;
        assert!(l.playlists()?.is_empty());
        assert!(l.playlist_tracks(id)?.is_empty());
        Ok(())
    }

    #[test]
    fn liked_history_downloads_and_local() -> anyhow::Result<()> {
        let db = Db::new();
        let l = &db.library;
        let a = track("a");
        let b = track("b");
        l.set_liked(&a, true)?;
        l.set_liked(&b, true)?;
        l.set_liked(&a, true)?;
        assert_eq!(l.liked()?, vec![b.clone(), a.clone()]);
        assert!(l.is_liked(&a.id));
        l.set_liked(&a, false)?;
        assert!(!l.is_liked(&a.id));
        l.record_play(&a)?;
        l.record_play(&a)?;
        l.record_play(&b)?;
        assert_eq!(l.recent(10)?, vec![b.clone(), a.clone()]);
        assert_eq!(l.most_played(1)?, vec![a.clone()]);
        l.set_download(&a, Path::new("/music/a.opus"))?;
        assert_eq!(l.download_path(&a.id), Some(PathBuf::from("/music/a.opus")));
        assert_eq!(l.downloads()?, vec![a.clone()]);
        l.remove_download(&a.id)?;
        assert!(l.downloads()?.is_empty());
        l.replace_local_tracks(&[a.clone(), b.clone()])?;
        l.replace_local_tracks(std::slice::from_ref(&b))?;
        assert_eq!(l.local_tracks()?, vec![b]);
        Ok(())
    }

    #[test]
    fn settings_and_session_roundtrip() -> anyhow::Result<()> {
        let db = Db::new();
        let l = &db.library;
        assert_eq!(l.load_settings(), Settings::default());
        assert!(l.load_session().is_none());
        let settings = Settings {
            volume: 42.0,
            scrobble_enabled: true,
            ..Settings::default()
        };
        l.save_settings(&settings)?;
        assert_eq!(Library::open(&db.path)?.load_settings(), settings);
        let session = Session {
            tracks: vec![track("x")],
            current: Some(0),
            position: 32.5,
        };
        l.save_session(&session)?;
        let restored = Library::open(&db.path)?.load_session().unwrap();
        assert_eq!(restored.tracks, session.tracks);
        assert_eq!(restored.current, session.current);
        assert_eq!(restored.position, session.position);
        l.conn
            .lock()
            .execute("UPDATE kv SET value='broken' WHERE key='settings'", [])?;
        assert_eq!(l.load_settings(), Settings::default());
        Ok(())
    }

    #[test]
    fn migrates_v1_and_keeps_links_on_partial_updates() -> anyhow::Result<()> {
        let db = Db::new();
        {
            let conn = Connection::open(&db.path)?;
            conn.execute_batch("ALTER TABLE tracks DROP COLUMN meta; PRAGMA user_version=1;")?;
        }
        let l = Library::open(&db.path)?;
        let mut t = track("x");
        t.artist_refs = vec![crate::model::ArtistRef {
            source: SourceKind::YouTube,
            name: "artist".into(),
            id: Some("UC1".into()),
        }];
        t.album_id = Some("MPRE1".into());
        l.upsert_tracks(&[t.clone()])?;
        let mut partial = track("x");
        partial.album_id = Some("MPRE2".into());
        l.upsert_tracks(&[partial])?;
        l.set_liked(&track("x"), true)?;
        let saved = l.liked()?.remove(0);
        assert_eq!(saved.artist_refs, t.artist_refs);
        assert_eq!(saved.album_id.as_deref(), Some("MPRE2"));
        Ok(())
    }

    #[test]
    fn top_artists_and_albums_by_period() -> anyhow::Result<()> {
        let db = Db::new();
        let l = &db.library;
        let mut a = track("a");
        a.artist = "Nina".into();
        a.album = Some("Blue".into());
        let mut b = track("b");
        b.artist = "nina".into();
        b.album = Some("blue".into());
        b.artwork_url = Some("https://img/b.jpg".into());
        let mut c = track("c");
        c.artist = "Otis".into();
        c.album = None;
        for t in [&a, &b, &b, &c, &c, &c] {
            l.record_play(t)?;
        }
        let old = now() - 90 * 24 * 3600 * 1000;
        l.conn.lock().execute(
            "INSERT INTO plays VALUES (?1,?2),(?1,?2),(?1,?2),(?1,?2)",
            params![a.id, old],
        )?;

        let all = l.top_artists(None, 10)?;
        assert_eq!(
            all.iter()
                .map(|t| (t.title.as_str(), t.plays))
                .collect::<Vec<_>>(),
            vec![("Nina", 7), ("Otis", 3)]
        );
        assert_eq!(all[0].track.id, a.id);
        assert_eq!(
            all[0].track.artwork_url.as_deref(),
            Some("https://img/b.jpg")
        );

        let month = now() - 30 * 24 * 3600 * 1000;
        let recent = l.top_artists(Some(month), 10)?;
        assert_eq!(
            recent
                .iter()
                .map(|t| (t.title.as_str(), t.plays))
                .collect::<Vec<_>>(),
            vec![("Otis", 3), ("nina", 3)]
        );
        assert_eq!(l.top_artists(Some(month), 1)?.len(), 1);

        let albums = l.top_albums(None, 10)?;
        assert_eq!(albums.len(), 1, "songs without an album are left out");
        assert_eq!(
            (
                albums[0].title.as_str(),
                albums[0].artist.as_str(),
                albums[0].plays
            ),
            ("Blue", "Nina", 7)
        );
        assert_eq!(l.top_albums(Some(month), 10)?[0].plays, 3);
        assert!(l.top_albums(Some(now() + 1000), 10)?.is_empty());
        Ok(())
    }
}
