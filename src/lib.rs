#![allow(dead_code)]

use std::error::Error;
use std::fmt;

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
    msg: Value,
}

impl ApiError {
    pub fn new(code: u16, msg: Value) -> ApiError {
        ApiError{ code, msg }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HTTP request returned bad code: {}\n", self.code)
    }
}

impl Error for ApiError {}

#[derive(Clone)]
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
        let res = self.hclient.get(url)
            .bearer_auth(self.key.clone())
            .send()
            .await?;

        let status = res.status().as_u16();
        let msg = res.text().await?;

        match status {
            200 | 201 => {
                Ok(msg)
            },

            _ => {
                if msg.is_empty() {
                    return Err(Box::new(ApiError::new(status, Value::Null)));
                }

                return Err(Box::new(ApiError::new(status, serde_json::from_str(msg.as_str())?)));
            },
        }
    }

    /// Get and parse a JSON response from a server
    pub async fn get(&self, url: String) -> Response<Value> {
        Ok(serde_json::from_str(self.get_raw(url).await?.as_str())?)
    }

    /// Post to a server
    pub async fn post_raw(&self, url: String, body: String) -> Response<String> {
        let res = self.hclient.post(url)
            .bearer_auth(self.key.clone())
            .body(body)
            .header("content-type", String::from("application/x-www-form-urlencoded"))
            .send()
            .await?;

        let status = res.status().as_u16();
        let msg = res.text().await?;

        

        match status {
            200 | 201 => {
                Ok(msg)
            },

            _ => {
                if msg.is_empty() {
                    return Err(Box::new(ApiError::new(status, Value::Null)));
                }

                return Err(Box::new(ApiError::new(status, serde_json::from_str(msg.as_str())?)));
            },
        }
    }

    /// Post to a server, returning json
    pub async fn post(&self, url: String, body: String) -> Response<Value> {
        Ok(serde_json::from_str(self.post_raw(url, body).await?.as_str())?)
    }

    /// Get a Lichess api endpoint
    pub async fn get_api(&self, endpoint: String) -> Response<Value> {
        self.get("https://lichess.org/api/".to_owned() + &endpoint).await
    }

    /// Post to a Lichess api endpoint, returning json
    pub async fn post_api(&self, endpoint: String, body: String) -> Response<Value> {
        self.post("https://lichess.org/api/".to_owned() + &endpoint, body).await
    }

    /// Post to a Lichess api endpoint
    pub async fn post_api_raw(&self, endpoint: String, body: String) -> Response<String> {
        self.post_raw("https://lichess.org/api/".to_owned() + &endpoint, body).await
    }

    /// Get the email of your account
    /// Requires `email:read` scope
    pub async fn email(&self) -> Response<String> {
        let res = self.get_api("account/email".into()).await?;

        if let Value::Object(err) = &res["error"] {
            return Err(format!("{:?}", err).into());
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

    /// Challenge the AI
    /// Requires `challenge:write` scope
    pub async fn ai(&self, level: i32, color: Color, clock: ClockSettings, initial: Option<String>) -> Response<String> {
        let mut body = format!("level={}", level);

        if color == Color::White {
            body.push_str("&color=white");
        } else {
            body.push_str("&color=black");
        }

        if clock.is_correspondence {
            body.push_str(format!("&days={}", clock.days).as_str());
        } else {
            body.push_str(format!("&clock.limit={}", clock.limit).as_str());
            body.push_str(format!("&clock.increment={}", clock.increment).as_str());
        }

        if let Some(fen) = initial {
            body.push_str(format!("&fen={}", fen).as_str());
        }
        
        let res = self.post_api(String::from("challenge/ai"), body).await?;

        if let Value::Object(err) = &res["error"] {
            return Err(format!("{:?}", err).into());
        }

        if let Value::String(id) = &res["id"] {
            return Ok(id.clone())
        }

        panic!("INTERNAL ERROR: something has gone horribly wrong (in client.rs: `fn ai`, line {})\n response: {:?}", line!(), res);
    }

    /// Create a seek
    /// Requires `board:play` scope
    pub async fn seek(&self, rated: bool, color: Color, clock: ClockSettings, initial: Option<String>) -> Response<Option<String>> {
        let mut body = String::from("{");

        match color {
            Color::White  => body.push_str("color=white"),
            Color::Black  => body.push_str("color=black"),
            Color::Random => body.push_str("color=random"),
        }

        if rated {
            body.push_str("&rated=true");
        }

        if clock.is_correspondence {
            body.push_str(format!("&days={}", clock.days).as_str());
        } else {
            body.push_str(format!("time={}", clock.limit).as_str());
            body.push_str(format!("increment={}", clock.increment).as_str());
        }

        if let Some(fen) = initial {
            body.push_str(format!("&fen={}", fen).as_str());
        }
        
        body.push_str("}\n");
        let res = self.post_api_raw(String::from("board/seek"), body).await?;

        if res.is_empty() {
            return Ok(None);
        } else {
            return Ok(Some(res));
        }
    }

    /// Make a move in a game
    /// Requires `board:play` scope
    pub async fn make_move(&self, id: &String, m: String, draw: bool) -> Response<bool> {
        let res = self.post_api(format!("board/game/{}/move/{}?offeringDraw={}", id, m, draw), String::new()).await?;
        
        if let Value::Object(err) = &res["error"] {
            return Err(format!("{:?}", err).into());
        }

        if let Value::Bool(ok) = &res["ok"] {
            return Ok(*ok);
        }

        panic!("INTERNAL ERROR: something has gone horribly wrong (in client.rs: `fn ai`, line {})", line!());
    }

    /// Resign a game
    /// Requires `board:play` scope
    pub async fn resign(&self, id: String) -> Response<bool> {
        let res = self.post_api(format!("board/game/{}/resign", id), String::new()).await?;
        
        if let Value::Object(err) = &res["error"] {
            return Err(format!("{:?}", err).into());
        }

        if let Value::Bool(ok) = &res["ok"] {
            return Ok(*ok);
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

    /// Get an ndjson stream from a server
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

    /// Get a listener to the Lichess events stream
    /// Requires `challenge:read bot:play board:play` scopes
    pub async fn events<T: DeserializeOwned>(&self) -> Response<impl Stream<Item = T>> {
        self.ndjson("https://lichess.org/api/stream/event".to_string()).await
    }

    /// Get a listener to a board
    /// Requires `board:play` scopre
    pub async fn board<T: DeserializeOwned>(&self, id: &String) -> Response<impl Stream<Item = T>> {
        println!("https://lichess.org/api/board/game/stream/{}", id);
        self.ndjson(format!("https://lichess.org/api/board/game/stream/{}", id)).await
    }

    // TODO: consider other ErrorKind's
    fn convert_err(e: reqwest::Error) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::Other, e)
    }
}