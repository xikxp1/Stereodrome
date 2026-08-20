use crate::data::{ResponseType, ScanStatus};
use crate::{Client, SubsonicError};

impl Client {
    /// reference: <http://www.subsonic.org/pages/api.jsp#getScanStatus>
    ///
    /// # Errors
    /// Returns an error when arguments are invalid, the request fails, or the response cannot be decoded.
    pub async fn get_scan_status(&self) -> Result<ScanStatus, SubsonicError> {
        let body = self.request("getScanStatus", None, None).await?;
        if let ResponseType::ScanStatus { scan_status } = body.data {
            Ok(scan_status)
        } else {
            Err(SubsonicError::Submarine(String::from(
                "expected type ScanStatus but found wrong type",
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::data::{OuterResponse, ResponseType};

    #[test]
    fn conversion_get_scan_status() -> anyhow::Result<()> {
        let response_body = r#"
            {
              "subsonic-response": {
                "status": "ok",
                "version": "1.16.1",
                "type": "navidrome",
                "serverVersion": "0.49.3 (8b93962f)",
                "scanStatus": {
                  "scanning": false,
                  "count": 18352,
                  "folderCount": 1205,
                  "lastScan": "2023-08-29T21:38:07.001850244Z"
                }
              }
            }"#;
        let response = serde_json::from_str::<OuterResponse>(response_body)?.inner;
        if let ResponseType::ScanStatus { scan_status } = response.data {
            check!(!scan_status.scanning);
            check_eq!(scan_status.count, Some(18352));
        } else {
            anyhow::bail!("wrong type: {response:?}");
        }
        Ok(())
    }
}
