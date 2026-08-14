use crate::{
    Client, Parameter, SubsonicError,
    data::{Info, ResponseType},
};

impl Client {
    /// reference: <http://www.subsonic.org/pages/api.jsp#updateShare>
    ///
    /// # Errors
    /// Returns an error when arguments are invalid, the request fails, or the response cannot be decoded.
    pub async fn update_share(
        &self,
        id: impl Into<String>,
        description: Option<impl Into<String>>,
        expires: Option<usize>,
    ) -> Result<Info, SubsonicError> {
        let mut paras = Parameter::new();
        paras.push("id", id);
        if let Some(desc) = description {
            paras.push("description", desc);
        }
        if let Some(expires) = expires {
            paras.push("expires", expires.to_string());
        }

        let body = self.request("updateShare", Some(paras), None).await?;
        if let ResponseType::Ping {} = body.data {
            Ok(body.info)
        } else {
            Err(SubsonicError::Submarine(String::from(
                "expected type Ping but found wrong type",
            )))
        }
    }
}
