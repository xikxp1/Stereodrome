use crate::{
    Client, SubsonicError,
    data::{ResponseType, User},
};

impl Client {
    /// reference: <http://www.subsonic.org/pages/api.jsp#getUsers>
    ///
    /// # Errors
    /// Returns an error when arguments are invalid, the request fails, or the response cannot be decoded.
    pub async fn get_users(&self) -> Result<Vec<User>, SubsonicError> {
        let body = self.request("getUsers", None, None).await?;
        if let ResponseType::Users { users } = body.data {
            Ok(users.user)
        } else {
            Err(SubsonicError::Submarine(String::from(
                "expected type Users but found wrong type",
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::data::{OuterResponse, ResponseType};

    #[test]
    fn conversion_get_users() -> anyhow::Result<()> {
        let response_body = r#"
{
  "subsonic-response": {
    "status": "ok",
    "version": "1.16.1",
    "type": "navidrome",
    "serverVersion": "0.49.3 (8b93962f)",
    "users": {
      "user": [
        {
          "username": "eppi",
          "scrobblingEnabled": true,
          "adminRole": false,
          "settingsRole": false,
          "downloadRole": true,
          "uploadRole": false,
          "playlistRole": false,
          "coverArtRole": false,
          "commentRole": false,
          "podcastRole": false,
          "streamRole": true,
          "jukeboxRole": false,
          "shareRole": false,
          "videoConversionRole": false
        }
      ]
    }
  }
}"#;
        let response = serde_json::from_str::<OuterResponse>(response_body)?.inner;
        println!("{response:?}");
        if let ResponseType::Users { users } = response.data {
            check_eq!(
                &users
                    .user
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("fixture item missing"))?
                    .username,
                "eppi"
            );
        } else {
            anyhow::bail!("wrong type: {response:?}");
        }
        Ok(())
    }
}
