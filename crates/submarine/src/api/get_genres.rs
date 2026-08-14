use crate::{
    Client, SubsonicError,
    data::{Genre, ResponseType},
};

impl Client {
    /// <http://www.subsonic.org/pages/api.jsp#getGenres>
    ///
    /// # Errors
    /// Returns an error when arguments are invalid, the request fails, or the response cannot be decoded.
    pub async fn get_genres(&self) -> Result<Vec<Genre>, SubsonicError> {
        let body = self.request("getGenres", None, None).await?;
        if let ResponseType::Genres { genres } = body.data {
            Ok(genres.genre)
        } else {
            Err(SubsonicError::Submarine(String::from(
                "expected type Genres but found wrong type",
            )))
        }
    }
}

mod tests {
    #[test]
    fn conversion_get_genres() -> anyhow::Result<()> {
        let response_body = r#"
            {
                "subsonic-response": {
                    "status": "ok",
                    "version": "1.16.1",
                    "type": "navidrome",
                    "serverVersion": "0.49.3 (8b93962f)",
                    "genres": {
                        "genre": [
                            {
                                "value": "Rock",
                                "songCount": 2860,
                                "albumCount": 211
                            },
                            {
                                "value": "Pop",
                                "songCount": 1336,
                                "albumCount": 87
                            }
                        ]
                    }
                }
            }"#;

        let response = serde_json::from_str::<crate::data::OuterResponse>(response_body)?.inner;
        if let crate::data::ResponseType::Genres { genres } = response.data {
            check_eq!(
                genres
                    .genre
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("fixture item missing"))?
                    .value,
                "Rock"
            );
            check_eq!(
                genres
                    .genre
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("fixture item missing"))?
                    .song_count,
                Some(2860)
            );
        } else {
            anyhow::bail!("wrong type");
        }
        Ok(())
    }
}
