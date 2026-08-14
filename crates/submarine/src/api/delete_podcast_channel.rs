use crate::{
    Client, Parameter, SubsonicError,
    data::{Info, ResponseType},
};

impl Client {
    /// reference: <http://www.subsonic.org/pages/api.jsp#deletePodcastChannel>
    ///
    /// # Errors
    /// Returns an error when arguments are invalid, the request fails, or the response cannot be decoded.
    pub async fn delete_podcast_channel(
        &self,
        id: impl Into<String>,
    ) -> Result<Info, SubsonicError> {
        let mut paras = Parameter::new();
        paras.push("id", id);

        let body = self
            .request("deletePodcastChannel", Some(paras), None)
            .await?;
        if let ResponseType::Ping {} = body.data {
            Ok(body.info)
        } else {
            Err(SubsonicError::Submarine(String::from(
                "expected type Ping but found wrong type",
            )))
        }
    }
}
