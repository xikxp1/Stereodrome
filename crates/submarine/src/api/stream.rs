use std::path::Path;

use tokio::io::AsyncWriteExt;

use crate::{Client, Parameter, SubsonicError};

impl Client {
    /// reference: <http://www.subsonic.org/pages/api.jsp#stream>
    ///
    /// # Errors
    /// Returns an error when the generated URL is invalid.
    pub fn stream_url(
        &self,
        id: impl Into<String>,
        max_bit_rate: Option<i32>,             // 0 for no limit
        format: Option<impl Into<String>>,     // file ending, raw for disable
        time_offset: Option<i64>,              // video only
        size: Option<impl Into<String>>,       // video only in "widthxheight" format
        estimate_content_length: Option<bool>, // restrict length
        converted: Option<bool>,               // video only
    ) -> Result<url::Url, url::ParseError> {
        let mut paras = Parameter::new();
        self.auth.add_parameter(&mut paras);
        paras.push("id", id);
        if let Some(bit_rate) = max_bit_rate {
            paras.push("maxBitRate", bit_rate.to_string());
        }
        if let Some(format) = format {
            paras.push("format", format);
        }
        if let Some(offset) = time_offset {
            paras.push("timeOffset", offset.to_string());
        }
        if let Some(size) = size {
            paras.push("size", size);
        }
        if let Some(content_length) = estimate_content_length {
            paras.push("estimateContentLength", content_length.to_string());
        }
        if let Some(converted) = converted {
            paras.push("converted", converted.to_string());
        }

        url::Url::parse_with_params(&format!("{}/rest/stream", self.server_url), paras.0)
    }

    /// reference: <http://www.subsonic.org/pages/api.jsp#stream>
    ///
    /// # Errors
    /// Returns an error when arguments are invalid, the request fails, or the response cannot be decoded.
    pub async fn stream(
        &self,
        id: impl Into<String>,
        max_bit_rate: Option<i32>,             // 0 for no limit
        format: Option<impl Into<String>>,     // file ending, raw for disable
        time_offset: Option<i64>,              // video only
        size: Option<impl Into<String>>,       // video only in "widthxheight" format
        estimate_content_length: Option<bool>, // restrict length
        converted: Option<bool>,               // video only
    ) -> Result<Vec<u8>, SubsonicError> {
        let result = match self
            .transport
            .get(self.stream_url(
                id,
                max_bit_rate,
                format,
                time_offset,
                size,
                estimate_content_length,
                converted,
            )?)
            .send()
            .await
        {
            Ok(result) => result,
            Err(e) => return Err(SubsonicError::Connection(e)),
        };

        let bytes = result.bytes().await?.into();
        Ok(bytes)
    }

    /// Streams a track response directly into `destination` and returns the
    /// number of bytes written.
    ///
    /// Unlike [`Self::stream`], this keeps memory bounded by the HTTP client's
    /// current body chunk. The caller owns atomic replacement and cleanup so it
    /// can coordinate cancellation with its cache metadata transaction.
    ///
    /// # Errors
    /// Returns an error when arguments are invalid, the request fails, or the response cannot be decoded.
    pub async fn stream_to_file(
        &self,
        id: impl Into<String>,
        max_bit_rate: Option<i32>,
        format: Option<impl Into<String>>,
        time_offset: Option<i64>,
        size: Option<impl Into<String>>,
        estimate_content_length: Option<bool>,
        converted: Option<bool>,
        destination: impl AsRef<Path>,
    ) -> Result<u64, SubsonicError> {
        let mut response = self
            .transport
            .get(self.stream_url(
                id,
                max_bit_rate,
                format,
                time_offset,
                size,
                estimate_content_length,
                converted,
            )?)
            .send()
            .await?
            .error_for_status()?;
        let mut file = tokio::fs::File::create(destination).await?;
        let mut byte_count = 0_u64;

        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk).await?;
            let chunk_len = u64::try_from(chunk.len()).map_err(|_| {
                SubsonicError::Submarine("stream response length exceeds u64".to_string())
            })?;
            byte_count = byte_count.checked_add(chunk_len).ok_or_else(|| {
                SubsonicError::Submarine("stream response length exceeds u64".to_string())
            })?;
        }
        file.flush().await?;
        Ok(byte_count)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use crate::{Client, auth::AuthBuilder};

    #[tokio::test]
    async fn create_stream_url() -> anyhow::Result<()> {
        let auth = AuthBuilder::new("peter", "v0.16.1")
            .salt_for_test("")
            .hashed("change_me_password");
        let client = Client::new("https://target.com", auth);
        let url =
            client.stream_url("testId", None, None::<&str>, None, None::<&str>, None, None)?;

        check_eq!(
            "https://target.com/rest/stream?u=peter&v=v0.16.1&c=submarine-lib&t=d4a5b2db9781fba37ec95f0312ade67a&s=&f=json&id=testId",
            &url.to_string()
        );
        Ok(())
    }

    #[tokio::test]
    async fn streams_chunked_response_directly_to_file() -> anyhow::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await?;
            let mut request = vec![0_u8; 4096];
            let _ = socket.read(&mut request).await?;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n",
                )
                .await
                ?;
            Ok::<(), anyhow::Error>(())
        });
        let auth = AuthBuilder::new("peter", "v0.16.1")
            .salt_for_test("")
            .hashed("change_me_password");
        let client = Client::new(&format!("http://{address}"), auth);
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let destination = std::env::temp_dir().join(format!(
            "submarine-stream-{}-{nonce}.bin",
            std::process::id()
        ));

        let byte_count = client
            .stream_to_file(
                "testId",
                None,
                Some("mp3"),
                None,
                None::<String>,
                None,
                None,
                &destination,
            )
            .await?;

        server.await??;
        check_eq!(byte_count, 11);
        check_eq!(tokio::fs::read(&destination).await?, b"hello world");
        let _ = tokio::fs::remove_file(destination).await;
        Ok(())
    }
}
