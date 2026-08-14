use crate::{
    Client, SubsonicError,
    data::{MusicFolders, ResponseType},
};

impl Client {
    /// reference: <http://www.subsonic.org/pages/api.jsp#getMusicFolders>
    ///
    /// # Errors
    /// Returns an error when arguments are invalid, the request fails, or the response cannot be decoded.
    pub async fn get_music_folders(&self) -> Result<MusicFolders, SubsonicError> {
        let body = self.request("getMusicFolders", None, None).await?;
        if let ResponseType::MusicFolders { music_folders } = body.data {
            Ok(music_folders)
        } else {
            Err(SubsonicError::Submarine(String::from(
                "expected type MusicFolders but found wrong type",
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::data::{OuterResponse, ResponseType};

    #[test]
    fn conversion_get_music_folders() -> anyhow::Result<()> {
        let response_body = r#"
            {
                "subsonic-response": {
                    "status": "ok",
                    "version": "1.16.1",
                    "type": "navidrome",
                    "serverVersion": "0.49.3 (8b93962f)",
                    "musicFolders": {
                        "musicFolder": [
                            {
                                "id": 0,
                                "name": "Music Library"
                            }
                        ]
                    }
                }
            }"#;
        let response = serde_json::from_str::<OuterResponse>(response_body)?.inner;
        if let ResponseType::MusicFolders { music_folders } = response.data {
            check_eq!(
                music_folders
                    .music_folder
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("fixture item missing"))?
                    .id,
                0
            );
        } else {
            anyhow::bail!("wrong type");
        }
        Ok(())
    }
}
