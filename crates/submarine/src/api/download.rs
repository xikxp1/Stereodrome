use crate::{Client, Parameter, SubsonicError};

impl Client {
    /// reference: <http://www.subsonic.org/pages/api.jsp#download>
    ///
    /// # Errors
    /// Returns an error when the generated URL is invalid.
    pub fn download_url(&self, id: impl Into<String>) -> Result<url::Url, url::ParseError> {
        let mut paras = Parameter::new();
        self.auth.add_parameter(&mut paras);
        paras.push("id", id);

        url::Url::parse_with_params(&format!("{}/rest/download", self.server_url), paras.0)
    }

    /// reference: <http://www.subsonic.org/pages/api.jsp#download>
    ///
    /// # Errors
    /// Returns an error when arguments are invalid, the request fails, or the response cannot be decoded.
    pub async fn download(&self, id: impl Into<String>) -> Result<Vec<u8>, SubsonicError> {
        let result = match self.transport.get(self.download_url(id)?).send().await {
            Ok(result) => result,
            Err(e) => return Err(SubsonicError::Connection(e)),
        };

        let bytes = result.bytes().await?.into();
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Client, auth::AuthBuilder};

    #[tokio::test]
    async fn create_download_url() -> anyhow::Result<()> {
        let auth = AuthBuilder::new("peter", "v0.16.1")
            .salt_for_test("")
            .hashed("change_me_password");
        let client = Client::new("https://target.com", auth);
        let url = client.download_url("testId")?;

        check_eq!(
            "https://target.com/rest/download?u=peter&v=v0.16.1&c=submarine-lib&t=d4a5b2db9781fba37ec95f0312ade67a&s=&f=json&id=testId",
            &url.to_string()
        );
        Ok(())
    }
}
