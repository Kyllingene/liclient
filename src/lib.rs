#![allow(dead_code)]

use std::{error::Error, fmt};

use tokio::io::AsyncBufReadExt;
use tokio_stream::wrappers::LinesStream;
use tokio_util::io::StreamReader;

use futures_util::stream::{Stream, StreamExt, TryStreamExt};

use serde_json::Value;
use serde::de::DeserializeOwned;

use chessboard::{Color, ClockSettings};

pub type Response<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug)]
pub struct ApiError {
    code: u16,
}

impl ApiError {
    pub fn new(code: u16) -> ApiError {
        ApiError{ code }
    }

    pub fn from_string(code: String) -> ApiError {
        ApiError{
            code: code.parse::<u16>().unwrap()
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HTTP request returned bad code: {}\n", self.code)
    }
}

impl Error for ApiError {}

pub struct Lichess {
    key: String,
    hclient: reqwest::Client,
}

impl Lichess {
    /// Make a new client with a Lichess API key
    pub fn new(key: String) -> Lichess {

        Lichess{
            key: key,
            hclient: reqwest::Client::new(),
        }
    }

    /// Get a plaintext response from a server
    pub async fn get_raw(&self, url: String) -> Response<String> {            
        let req = self.hclient.get(url)
            .bearer_auth(self.key.clone())
            .build()?;

        let res = self.hclient.execute(req).await?;

        match res.status().into() {
            200 | 201 | 400 | 401 => {
                Ok(res.text().await?)
            },

            _ => return Err(Box::new(ApiError::new(res.status().as_u16()))),
        }
    }

    /// Get and parse a JSON response from a server
    pub async fn get(&self, url: String) -> Response<Value> {
        Ok(serde_json::from_str(self.get_raw(url).await?.as_str())?)
    }

    /// Post to a server
    pub async fn post(&self, url: String, body: String) -> Response<Value> {
        // let req = hyper::Request::builder()
        //     .method(hyper::Method::POST)
        //     .uri(url)
        //     .header("Authorization", self.key.clone())
        //     .header("content-type", "application/x-www-form-urlencoded")
        //     .body(hyper::Body::from(body))?;

        let req = self.hclient.post(url)
            .bearer_auth(self.key.clone())
            .body(body)
            .build()?;

        let res = self.hclient.execute(req).await?;

        match res.status().into() {
            200 | 201 | 400 | 401 => {
    
                Ok(serde_json::from_str(res.text().await?.as_str())?)
            },

            _ => return Err(Box::new(ApiError::new(res.status().as_u16()))),
        }
    }

    /// Get a Lichess api endpoint
    pub async fn get_api(&self, endpoint: String) -> Response<Value> {
        self.get("https://lichess.org/api/".to_owned() + &endpoint).await
    }

    /// Post to a Lichess api endpoint
    pub async fn post_api(&self, endpoint: String, body: String) -> Response<Value> {
        self.post("https://lichess.org/api/".to_owned() + &endpoint, body).await
    }

    /// Get the email of your account
    /// Requires `email:read` scope
    pub async fn email(&self) -> Response<String> {
        let res = self.get_api("account/email".into()).await?;

        if let Value::String(err) = &res["error"] {
            return Err(String::from(err).into());
        }

        if let Value::String(email) = &res["email"] {
            return Ok(email.clone())
        }

        // TODO: can this ever actually be reached? if so, replace; else, remove
        panic!("INTERNAL ERROR: something has gone horribly wrong (in client.rs: `fn email`, line {})", line!());
    }

    /// Get your account details
    /// Requires no scopes
    pub async fn account(&self) -> Response<Value> {
        self.get_api("account".to_string()).await
    }

    /// Check authentication by attempting to get account details
    /// Requires no scopes
    pub async fn auth(&self) -> Response<bool> {
        self.account().await?;

        // If the previous call didn't fail, then we must've gotten our account info back, which means we are authenticated
        Ok(true)
    }

    /// Challenge the AI
    /// Requires `challenge:write` scope
    pub async fn ai(&self, level: i32, color: Color, clock: ClockSettings, initial: Option<String>) -> Response<String> {
        let mut body = String::from("{");

        body.push_str(format!("level={}", level).as_str());

        if color == Color::White {
            body.push_str("&color=white");
        } else {
            body.push_str("&color=black");
        }

        if clock.is_correspondence {
            body.push_str(format!("&days={}", clock.days).as_str());
        } else {
            body.push_str(format!("clock.limit={}", clock.limit).as_str());
            body.push_str(format!("clock.increment={}", clock.increment).as_str());
        }

        if let Some(fen) = initial {
            body.push_str(format!("&fen={}", fen).as_str());
        }
        
        body.push_str("}\n");
        let res = self.post_api(String::from("api/challenge/ai"), body).await?;

        if let Value::String(err) = &res["error"] {
            return Err(String::from(err).into());
        }

        if let Value::String(id) = &res["id"] {
            return Ok(id.clone())
        }

        panic!("INTERNAL ERROR: something has gone horribly wrong (in client.rs: `fn ai`, line {})", line!());
    }

    /// Get a stream from a server
    pub async fn stream(&self, url: String) -> Response<impl Stream<Item = String>> {
        let res = self.hclient.get(url)
            .bearer_auth(self.key.clone())
            .send()
            .await?
            .bytes_stream();

        Ok(Box::pin(
            LinesStream::new(StreamReader::new(res.map_err(Lichess::convert_err)).lines()).filter_map(|l| async move {
                let line = l.ok()?;
                if line.is_empty() {
                    None
                } else {
                    Some(line)
                }
            })
        ))
    }

    pub async fn ndjson<T: DeserializeOwned>(&self, url: String) -> Response<impl Stream<Item = T>> {
        let res = self.hclient.get(url)
            .bearer_auth(self.key.clone())
            .send()
            .await?
            .bytes_stream();

        Ok(Box::pin(
            LinesStream::new(StreamReader::new(res.map_err(Lichess::convert_err)).lines()).filter_map(|l| async move {
                let line = l.ok()?;
                if line.is_empty() {
                    None
                } else {
                    serde_json::from_str(line.as_str()).ok()?
                }
            })
        ))
    }

    // TODO: consider other ErrorKind's
    fn convert_err(e: reqwest::Error) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::Other, e)
    }
}