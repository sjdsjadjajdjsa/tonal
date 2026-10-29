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

const COLUMNS: &str =
    "t.id,t.source,t.source_id,t.title,t.artist,t.album,t.duration_secs,t.artwork_url,t.play_url";

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn upsert(conn: &Connection, tracks: &[Track]) -> anyhow::Result<()> {
    let mut stmt = conn.prepare_cached("INSERT INTO tracks VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
        ON CONFLICT(id) DO UPDATE SET source=excluded.source,source_id=excluded.source_id,
        title=excluded.title,artist=excluded.artist,album=excluded.album,
        duration_secs=excluded.duration_secs,artwork_url=excluded.artwork_url,play_url=excluded.play_url")?;
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
            t.play_url
        ])?;
    }
    Ok(())
}

fn row_track(row: &rusqlite::Row<'_>) -> rusqlite::Result<Track> {
    let source: String = row.get(1)?;
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
            version <= 1,
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
}
