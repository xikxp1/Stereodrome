use crate::{
    Client, SubsonicError,
    data::{InternetRadioStation, ResponseType},
};

impl Client {
    /// reference: <http://www.subsonic.org/pages/api.jsp#getInternetRadioStations>
    ///
    /// # Errors
    /// Returns an error when arguments are invalid, the request fails, or the response cannot be decoded.
    pub async fn get_internet_radio_stations(
        &self,
    ) -> Result<Vec<InternetRadioStation>, SubsonicError> {
        let body = self.request("getInternetRadioStations", None, None).await?;
        if let ResponseType::InternetRadionStations {
            internet_radio_stations,
        } = body.data
        {
            Ok(internet_radio_stations.internet_radio_station)
        } else {
            Err(SubsonicError::Submarine(String::from(
                "expected type InternetRadioStations but found wrong type",
            )))
        }
    }
}

//TODO add better test
#[cfg(test)]
mod tests {
    use crate::data::{OuterResponse, ResponseType};

    #[test]
    fn conversion_empty_get_internet_radio_stations() -> anyhow::Result<()> {
        let response_body = r#"
{
    "subsonic-response": {
        "status": "ok",
        "version": "1.16.1",
        "type": "navidrome",
        "serverVersion": "0.49.3 (8b93962f)",
        "internetRadioStations": {}
    }
}"#;
        let response = serde_json::from_str::<OuterResponse>(response_body)?.inner;
        if let ResponseType::InternetRadionStations {
            internet_radio_stations,
        } = response.data
        {
            check_eq!(internet_radio_stations.internet_radio_station, vec![]);
        } else {
            anyhow::bail!("wrong type");
        }
        Ok(())
    }
}
