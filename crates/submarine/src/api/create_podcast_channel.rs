use crate::{
    Client, Parameter, SubsonicError,
    data::{Info, ResponseType},
};

impl Client {
    /// reference: <http://www.subsonic.org/pages/api.jsp#createPodcastChannel>
    ///
    /// # Errors
    /// Returns an error when arguments are invalid, the request fails, or the response cannot be decoded.
    pub async fn create_podcast_channel(
        &self,
        url: impl Into<String>,
    ) -> Result<Info, SubsonicError> {
        let mut paras = Parameter::new();
        paras.push("url", url);

        let body = self
            .request("createPodcastChannel", Some(paras), None)
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
