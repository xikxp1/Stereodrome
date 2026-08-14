use crate::{
    Client, SubsonicError,
    data::{Info, ResponseType},
};

impl Client {
    /// pings server and sends its [Info]<br>
    /// reference: <http://www.subsonic.org/pages/api.jsp#ping>
    ///
    /// # Errors
    /// Returns an error when arguments are invalid, the request fails, or the response cannot be decoded.
    pub async fn ping(&self) -> Result<Info, SubsonicError> {
        let body = self.request("ping", None, None).await?;
        if let ResponseType::Ping {} = body.data {
            Ok(body.info)
        } else {
            Err(SubsonicError::Submarine(String::from(
                "expected type Ping but found wrong type",
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::data::{OuterResponse, ResponseType, Status};

    #[test]
    fn ping_convert() -> anyhow::Result<()> {
        let response_txt = r#"
            {
              "subsonic-response": {
                "status": "ok",
                "version": "1.16.1",
                "type": "navidrome",
                "serverVersion": "0.49.3 (8b93962f)"
              }
            }"#;
        let info = serde_json::from_str::<OuterResponse>(response_txt)?
            .inner
            .info;
        check_eq!(info.status, Status::Ok);
        check_eq!(info.version, String::from("1.16.1"));
        check_eq!(info.r#type, Some(String::from("navidrome")));
        Ok(())
    }

    #[test]
    fn convert_error() -> anyhow::Result<()> {
        let response_txt = r#"
            {
              "subsonic-response": {
                "status": "failed",
                "version": "1.16.1",
                "type": "navidrome",
                "serverVersion": "0.49.3 (8b93962f)",
                "error": {
                  "code": 40,
                  "message": "Wrong username or password"
                }
              }
            }"#;
        let response = serde_json::from_str::<OuterResponse>(response_txt)?.inner;

        check_eq!(response.info.status, Status::Error);
        check_eq!(response.info.version, String::from("1.16.1"));
        if let ResponseType::Error { error } = response.data {
            check_eq!(error.code, 40);
            check_eq!(error.message, String::from("Wrong username or password"));
        } else {
            anyhow::bail!("wrong type");
        }
        Ok(())
    }
}
