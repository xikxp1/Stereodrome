use crate::{
    Client, Parameter, SubsonicError,
    data::{Info, ResponseType},
};

impl Client {
    /// reference: <http://www.subsonic.org/pages/api.jsp#changePassword>
    ///
    /// # Errors
    /// Returns an error when arguments are invalid, the request fails, or the response cannot be decoded.
    pub async fn change_password(
        &self,
        username: impl Into<String>,
        password: impl Into<String>, // clear or in hex with prefix 'enc:'
    ) -> Result<Info, SubsonicError> {
        let mut paras = Parameter::new();
        paras.push("username", username);
        paras.push("password", password);

        let body = self.request("changePassword", Some(paras), None).await?;
        if let ResponseType::Ping {} = body.data {
            Ok(body.info)
        } else {
            Err(SubsonicError::Submarine(String::from(
                "expected type Ping but found wrong type",
            )))
        }
    }
}
