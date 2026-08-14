use crate::{
    Client, SubsonicError,
    data::{License, ResponseType},
};

impl Client {
    /// reference: <http://www.subsonic.org/pages/api.jsp#getLicense>
    ///
    /// # Errors
    /// Returns an error when arguments are invalid, the request fails, or the response cannot be decoded.
    pub async fn get_license(&self) -> Result<License, SubsonicError> {
        let body = self.request("getLicense", None, None).await?;
        if let ResponseType::License { license } = body.data {
            Ok(license)
        } else {
            Err(SubsonicError::Submarine(String::from(
                "expected type License but found wrong type",
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::data::{OuterResponse, ResponseType};

    #[test]
    fn conversion_get_license() -> anyhow::Result<()> {
        let response_body = r#"
            {
              "subsonic-response": {
                "status": "ok",
                "version": "1.16.1",
                "type": "navidrome",
                "serverVersion": "0.49.3 (8b93962f)",
                "license": {
                  "valid": true
                }
              }
            }"#;
        let response = serde_json::from_str::<OuterResponse>(response_body)?.inner;
        if let ResponseType::License { license } = response.data {
            check!(license.valid);
        } else {
            anyhow::bail!("wrong type");
        }
        Ok(())
    }
}
