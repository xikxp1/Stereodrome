use crate::{
    Client, SubsonicError,
    data::{ResponseType, Share},
};

impl Client {
    /// reference: <http://www.subsonic.org/pages/api.jsp#getShares>
    ///
    /// # Errors
    /// Returns an error when arguments are invalid, the request fails, or the response cannot be decoded.
    pub async fn get_shares(&self) -> Result<Vec<Share>, SubsonicError> {
        let body = self.request("getShares", None, None).await?;
        if let ResponseType::Shares { shares } = body.data {
            Ok(shares.share)
        } else {
            Err(SubsonicError::Submarine(String::from(
                "expected type Shares but found wrong type",
            )))
        }
    }
}

//TODO test
