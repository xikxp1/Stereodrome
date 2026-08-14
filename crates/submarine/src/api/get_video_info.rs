use crate::data::{ResponseType, VideoInfo};
use crate::{Client, Parameter, SubsonicError};

impl Client {
    /// reference: <http://www.subsonic.org/pages/api.jsp#getVideoInfo>
    ///
    /// # Errors
    /// Returns an error when arguments are invalid, the request fails, or the response cannot be decoded.
    pub async fn get_video_info(&self, id: impl Into<String>) -> Result<VideoInfo, SubsonicError> {
        let mut paras = Parameter::new();
        paras.push("id", id);

        let body = self.request("getVideoInfo", Some(paras), None).await?;
        if let ResponseType::VideoInfo { video_info } = body.data {
            Ok(video_info)
        } else {
            Err(SubsonicError::Submarine(String::from(
                "expected type VideoInfo but found wrong type",
            )))
        }
    }
}

// TODO add test
