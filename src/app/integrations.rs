use super::{controller::Controller, playback::now};
use futures::{StreamExt, stream};

impl Controller {
    async fn import_tracks(
        &self,
        name: String,
        origin: Option<String>,
        tracks: Vec<crate::integrations::spotify::SpotifyTrack>,
    ) -> anyhow::Result<()> {
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
        let id = self.library.create_playlist(&name, origin.as_deref())?;
        self.library.add_to_playlist(id, &tracks)?;
        self.refresh_playlists();
        self.go(super::browse::Nav::Playlist(id));
        self.status(format!(
            "Imported {} tracks · {failures} unmatched",
            tracks.len()
        ));
        Ok(())
    }
    pub(super) fn import_public(&self, url: String) {
        let this = self.clone();
        self.status("Fetching Spotify playlist…");
        tokio::spawn(async move {
            if let Some((name, tracks)) =
                this.result(crate::integrations::spotify::fetch_public(&url).await)
            {
                this.result(this.import_tracks(name, Some(url), tracks).await);
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
                    match crate::integrations::spotify::refresh(&s.spotify_client_id, token).await {
                        Ok(auth) => auth,
                        Err(_) => crate::integrations::spotify::login(&s.spotify_client_id).await?,
                    }
                } else {
                    crate::integrations::spotify::login(&s.spotify_client_id).await?
                };
                {
                    let mut state = this.state.lock();
                    state.settings.spotify_refresh_token = Some(auth.refresh_token.clone());
                    this.library.save_settings(&state.settings)?;
                }
                let liked = crate::integrations::spotify::liked_tracks(&auth).await?;
                this.import_tracks("Spotify · Liked songs".into(), None, liked)
                    .await?;
                for list in crate::integrations::spotify::my_playlists(&auth).await? {
                    if auth.expires_at <= now() + 60 {
                        auth = crate::integrations::spotify::refresh(
                            &s.spotify_client_id,
                            &auth.refresh_token,
                        )
                        .await?;
                        let mut state = this.state.lock();
                        state.settings.spotify_refresh_token = Some(auth.refresh_token.clone());
                        this.library.save_settings(&state.settings)?;
                    }
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
                    let tracks =
                        crate::integrations::spotify::playlist_tracks(&auth, &list.id).await?;
                    this.import_tracks(
                        list.name,
                        Some(format!("https://open.spotify.com/playlist/{}", list.id)),
                        tracks,
                    )
                    .await?;
                }
                Ok(())
            }
            .await;
            this.result(result);
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
