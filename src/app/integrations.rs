use super::{controller::Controller, playback::now};
use crate::integrations::spotify::{self, SpotifyAuth, SpotifyTrack, Superseded};
use crate::model::{Playlist, Track};
use futures::{StreamExt, stream};

const LIKED_NAME: &str = "Spotify · Liked songs";

#[derive(Debug, PartialEq)]
enum SyncPlan {
    Unchanged,
    Append(usize),
    Replace,
}

fn sync_plan(existing: &[String], new: &[String]) -> SyncPlan {
    if existing == new {
        SyncPlan::Unchanged
    } else if new.starts_with(existing) {
        SyncPlan::Append(existing.len())
    } else {
        SyncPlan::Replace
    }
}

fn existing_playlist<'a>(
    playlists: &'a [Playlist],
    origin: &str,
    legacy_name: Option<&str>,
) -> Option<&'a Playlist> {
    playlists
        .iter()
        .find(|p| {
            p.origin.as_deref().is_some_and(|o| {
                o == origin || spotify::public_origin(o).ok().as_deref() == Some(origin)
            })
        })
        .or_else(|| {
            let name = legacy_name?;
            playlists
                .iter()
                .find(|p| p.origin.is_none() && p.name == name)
        })
}

impl Controller {
    fn save_spotify_token(&self, refresh_token: &str) -> anyhow::Result<()> {
        let mut state = self.state.lock();
        state.settings.spotify_refresh_token = Some(refresh_token.to_string());
        self.library.save_settings(&state.settings)
    }

    async fn fresh_spotify_auth(
        &self,
        client_id: &str,
        auth: &mut SpotifyAuth,
    ) -> anyhow::Result<()> {
        if auth.expires_at > now() + 60 {
            return Ok(());
        }
        *auth = spotify::refresh(client_id, &auth.refresh_token).await?;
        self.save_spotify_token(&auth.refresh_token)
    }

    fn sync_playlist_tracks(&self, playlist: &Playlist, tracks: &[Track]) -> anyhow::Result<()> {
        let existing: Vec<String> = self
            .library
            .playlist_tracks(playlist.id)?
            .into_iter()
            .map(|t| t.id)
            .collect();
        let new: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
        let plan = if existing.len() == playlist.track_count {
            sync_plan(&existing, &new)
        } else {
            SyncPlan::Replace
        };
        match plan {
            SyncPlan::Unchanged => self.library.upsert_tracks(tracks),
            SyncPlan::Append(from) => {
                self.library.upsert_tracks(&tracks[..from])?;
                self.library.add_to_playlist(playlist.id, &tracks[from..])
            }
            SyncPlan::Replace => self.library.replace_playlist_tracks(playlist.id, tracks),
        }
    }

    async fn import_tracks(
        &self,
        name: String,
        origin: String,
        legacy_name: Option<&str>,
        tracks: Vec<SpotifyTrack>,
    ) -> anyhow::Result<Option<i64>> {
        let total = tracks.len();
        let mut done = 0;
        let mut matched = Vec::new();
        let mut failures = 0;
        let tasks = stream::iter(tracks.into_iter().enumerate().map(|(index, t)| async move {
            let result = crate::sources::match_track(
                crate::model::SourceKind::YouTube,
                &t.title,
                &t.artist,
                t.duration_secs,
            )
            .await;
            (index, t, result)
        }))
        .buffer_unordered(4);
        futures::pin_mut!(tasks);
        while let Some((index, metadata, result)) = tasks.next().await {
            done += 1;
            match result {
                Ok(Some(mut t)) => {
                    if t.album.is_none() {
                        t.album = metadata.album;
                    }
                    if t.artwork_url.is_none() {
                        t.artwork_url = metadata.artwork_url;
                    }
                    matched.push((index, t));
                }
                Ok(None) | Err(_) => failures += 1,
            }
            self.status(format!(
                "Matched {}/{total} · {done}/{total} processed",
                matched.len()
            ));
        }
        matched.sort_by_key(|(i, _)| *i);
        let tracks = matched.into_iter().map(|(_, t)| t).collect::<Vec<_>>();
        if tracks.is_empty() {
            self.status(format!("Skipped {name} · no tracks matched"));
            return Ok(None);
        }
        let playlists = self.library.playlists()?;
        let id = match existing_playlist(&playlists, &origin, legacy_name) {
            Some(playlist) => {
                self.sync_playlist_tracks(playlist, &tracks)?;
                playlist.id
            }
            None => {
                let id = self.library.create_playlist(&name, Some(&origin))?;
                self.library.add_to_playlist(id, &tracks)?;
                id
            }
        };
        self.refresh_playlists();
        if self.state.lock().playlist == Some(id) {
            self.playlist(id);
        }
        self.status(format!(
            "Imported {name} · {} tracks · {failures} unmatched",
            tracks.len()
        ));
        Ok(Some(id))
    }
    pub(super) fn import_public(&self, url: String) {
        let Some(origin) = self.result(spotify::public_origin(&url)) else {
            return;
        };
        let this = self.clone();
        let from = self.current_nav();
        self.status("Fetching Spotify playlist…");
        tokio::spawn(async move {
            if let Some((name, tracks)) = this.result(spotify::fetch_public(&url).await)
                && let Some(Some(id)) =
                    this.result(this.import_tracks(name, origin, None, tracks).await)
            {
                let target = super::browse::Nav::Playlist(id);
                if this.current_nav() == from && from.as_ref() != Some(&target) {
                    this.go(target);
                }
            }
        });
    }
    pub(super) fn spotify_login(&self) {
        let this = self.clone();
        let s = self.state.lock().settings.clone();
        self.status("Opening Spotify login…");
        tokio::spawn(async move {
            let result: anyhow::Result<()> = async {
                let mut auth = if let Some(token) = s.spotify_refresh_token.as_deref() {
                    match spotify::refresh(&s.spotify_client_id, token).await {
                        Ok(auth) => auth,
                        Err(_) => spotify::login(&s.spotify_client_id).await?,
                    }
                } else {
                    spotify::login(&s.spotify_client_id).await?
                };
                this.save_spotify_token(&auth.refresh_token)?;
                let liked = spotify::liked_tracks(&auth).await?;
                this.import_tracks(
                    LIKED_NAME.into(),
                    spotify::LIKED_ORIGIN.into(),
                    Some(LIKED_NAME),
                    liked,
                )
                .await?;
                this.fresh_spotify_auth(&s.spotify_client_id, &mut auth)
                    .await?;
                let mut imported = 0;
                let mut skipped = 0;
                for list in spotify::my_playlists(&auth).await? {
                    this.fresh_spotify_auth(&s.spotify_client_id, &mut auth)
                        .await?;
                    this.status(format!(
                        "Importing {} · {} tracks{}",
                        list.name,
                        list.track_count,
                        if list.image_url.is_some() {
                            " · with artwork"
                        } else {
                            ""
                        }
                    ));
                    let tracks = match spotify::playlist_tracks(&auth, &list.id).await {
                        Ok(tracks) => tracks,
                        Err(e) => {
                            log::warn!(
                                "Skipping Spotify playlist {} ({}): {e:#}",
                                list.name,
                                list.id
                            );
                            skipped += 1;
                            continue;
                        }
                    };
                    if this
                        .import_tracks(list.name, spotify::playlist_origin(&list.id), None, tracks)
                        .await?
                        .is_some()
                    {
                        imported += 1;
                    }
                }
                this.status(if skipped > 0 {
                    format!(
                        "Spotify import finished · {imported} playlists · {skipped} unavailable"
                    )
                } else {
                    format!("Spotify import finished · {imported} playlists")
                });
                Ok(())
            }
            .await;
            match result {
                Err(e) if e.downcast_ref::<Superseded>().is_some() => {}
                result => {
                    this.result(result);
                }
            }
        });
    }
    pub(super) fn lastfm_login(&self) {
        let this = self.clone();
        let s = self.state.lock().settings.clone();
        self.status("Approve Tonal in your browser…");
        tokio::spawn(async move {
            if let Some(session) = this.result(
                crate::integrations::scrobble::lastfm_login(
                    &s.lastfm_api_key,
                    &s.lastfm_api_secret,
                )
                .await,
            ) {
                {
                    this.state.lock().settings.lastfm_session = Some(session);
                }
                this.save_settings();
                this.status("Connected to Last.fm");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn playlist(id: i64, name: &str, origin: Option<&str>) -> Playlist {
        Playlist {
            id,
            name: name.into(),
            track_count: 0,
            origin: origin.map(str::to_string),
        }
    }

    #[test]
    fn sync_plan_keeps_appends_or_replaces() {
        assert_eq!(
            sync_plan(&ids(&["a", "b"]), &ids(&["a", "b"])),
            SyncPlan::Unchanged
        );
        assert_eq!(
            sync_plan(&ids(&["a"]), &ids(&["a", "b"])),
            SyncPlan::Append(1)
        );
        assert_eq!(sync_plan(&[], &ids(&["a"])), SyncPlan::Append(0));
        assert_eq!(
            sync_plan(&ids(&["b", "a"]), &ids(&["a", "b"])),
            SyncPlan::Replace
        );
        assert_eq!(
            sync_plan(&ids(&["a", "b"]), &ids(&["a"])),
            SyncPlan::Replace
        );
    }

    #[test]
    fn reimport_finds_playlist_by_origin() {
        let origin = spotify::playlist_origin("abc");
        let lists = vec![
            playlist(1, LIKED_NAME, None),
            playlist(
                2,
                "Mine",
                Some("https://open.spotify.com/playlist/abc?si=x"),
            ),
            playlist(3, "Other", Some(&spotify::playlist_origin("zzz"))),
            playlist(4, "Liked", Some(spotify::LIKED_ORIGIN)),
        ];
        assert_eq!(
            existing_playlist(&lists, &origin, None).map(|p| p.id),
            Some(2)
        );
        assert_eq!(
            existing_playlist(&lists, spotify::LIKED_ORIGIN, Some(LIKED_NAME)).map(|p| p.id),
            Some(4)
        );
        assert_eq!(
            existing_playlist(&lists[..1], spotify::LIKED_ORIGIN, Some(LIKED_NAME)).map(|p| p.id),
            Some(1)
        );
        assert!(existing_playlist(&lists, &spotify::playlist_origin("new"), None).is_none());
    }
}
