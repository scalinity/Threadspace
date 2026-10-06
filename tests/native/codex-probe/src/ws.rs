//! Passive Codex app-server observer handshake (SPEC §12.4): an RFC 6455
//! client written by hand over a byte stream (AF_UNIX in use), followed by the
//! JSON-RPC `initialize` → response → `initialized` sequence and at most one
//! bounded `thread/loaded/list`. Client frames are masked; server frames must
//! not be. Server requests are recorded by method name and never answered, and
//! no other method is ever sent.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
/// Largest server message accepted; the observer reads only small responses.
pub const MAX_MESSAGE: usize = 1 << 20;
const MAX_UPGRADE_RESPONSE: usize = 16 * 1024;
const MAX_LOGGED: usize = 64;

pub const CLIENT_NAME: &str = "threadspace_m0c_probe";
pub const CLIENT_TITLE: &str = "Threadspace M0C passive probe";

pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut state: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut message = data.to_vec();
    let bit_length = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());
    let (blocks, _) = message.as_chunks::<64>();
    for block in blocks {
        let mut words = [0u32; 80];
        let (quads, _) = block.as_chunks::<4>();
        for (index, quad) in quads.iter().enumerate() {
            words[index] = u32::from_be_bytes(*quad);
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (index, word) in words.iter().enumerate() {
            let (f, k) = match index {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut digest = [0u8; 20];
    let (quads, _) = digest.as_chunks_mut::<4>();
    for (quad, value) in quads.iter_mut().zip(state) {
        *quad = value.to_be_bytes();
    }
    digest
}

pub fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let symbol = |value: u32| ALPHABET[(value & 63) as usize] as char;
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let bytes = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2]);
        out.push(symbol(n >> 18));
        out.push(symbol(n >> 12));
        out.push(if chunk.len() > 1 { symbol(n >> 6) } else { '=' });
        out.push(if chunk.len() > 2 { symbol(n) } else { '=' });
    }
    out
}

/// `Sec-WebSocket-Accept` for a client key (RFC 6455 §4.2.2).
pub fn accept_for(key: &str) -> String {
    base64(&sha1(format!("{key}{GUID}").as_bytes()))
}

/// Random bytes from v4 UUIDs, skipping the fixed version/variant bytes.
fn random_bytes(count: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(count);
    while out.len() < count {
        let bytes = *uuid::Uuid::new_v4().as_bytes();
        out.extend(
            bytes
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != 6 && *index != 8)
                .map(|(_, byte)| *byte),
        );
    }
    out.truncate(count);
    out
}

pub fn encode_frame(opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 14);
    out.push(0x80 | opcode);
    let length = payload.len();
    if length < 126 {
        out.push(0x80 | length as u8);
    } else if length <= usize::from(u16::MAX) {
        out.push(0x80 | 126);
        out.extend_from_slice(&(length as u16).to_be_bytes());
    } else {
        out.push(0x80 | 127);
        out.extend_from_slice(&(length as u64).to_be_bytes());
    }
    out.extend_from_slice(&mask);
    out.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % 4]),
    );
    out
}

#[derive(Debug)]
pub enum WsError {
    Io(io::Error),
    Timeout,
    UpgradeRejected(String),
    Protocol(&'static str),
    TooLarge(u64),
    Closed(Option<u16>),
}

impl WsError {
    pub fn code(&self) -> String {
        match self {
            Self::Io(error) => format!("IO:{:?}", error.kind()),
            Self::Timeout => "TIMEOUT".into(),
            Self::UpgradeRejected(status) => format!("UPGRADE_REJECTED:{status}"),
            Self::Protocol(reason) => format!("PROTOCOL:{reason}"),
            Self::TooLarge(length) => format!("MESSAGE_TOO_LARGE:{length}"),
            Self::Closed(code) => format!("CLOSED:{code:?}"),
        }
    }
}

impl From<io::Error> for WsError {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => Self::Timeout,
            _ => Self::Io(error),
        }
    }
}

pub struct Upgrade {
    pub status_line: String,
    pub accept_valid: bool,
    pub max_unfragmented_message_bytes: Option<String>,
}

pub enum Message {
    Text(String),
    Binary(usize),
    Close(Option<u16>),
}

pub struct Connection<S: Read + Write> {
    stream: S,
    pub frames_sent: usize,
    pub pongs_sent: usize,
}

impl<S: Read + Write> Connection<S> {
    /// HTTP/1.1 upgrade exactly as the released daemon client requests it
    /// (`ws://localhost/`). Reads the response byte by byte so no frame bytes
    /// are consumed with the headers.
    pub fn client(mut stream: S) -> Result<(Self, Upgrade), WsError> {
        let key = base64(&random_bytes(16));
        let request = format!(
            "GET / HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {key}\r\n\r\n"
        );
        stream.write_all(request.as_bytes())?;
        stream.flush()?;
        let mut response = Vec::new();
        let mut byte = [0u8; 1];
        while !response.ends_with(b"\r\n\r\n") {
            if response.len() >= MAX_UPGRADE_RESPONSE {
                return Err(WsError::Protocol("upgrade response too large"));
            }
            match stream.read(&mut byte)? {
                0 => return Err(WsError::Protocol("closed during upgrade")),
                _ => response.push(byte[0]),
            }
        }
        let text = String::from_utf8_lossy(&response).into_owned();
        let mut lines = text.split("\r\n");
        let status_line = lines.next().unwrap_or_default().to_string();
        if status_line.split_whitespace().nth(1) != Some("101") {
            return Err(WsError::UpgradeRejected(status_line));
        }
        let headers: Vec<(String, String)> = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
            .collect();
        let header = |name: &str| {
            headers
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        let upgrade_ok =
            header("upgrade").is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
        let connection_ok = header("connection").is_some_and(|value| {
            value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
        });
        let accept_valid =
            header("sec-websocket-accept").as_deref() == Some(accept_for(&key).as_str());
        if !(upgrade_ok && connection_ok && accept_valid) {
            return Err(WsError::Protocol("invalid upgrade headers"));
        }
        Ok((
            Self {
                stream,
                frames_sent: 0,
                pongs_sent: 0,
            },
            Upgrade {
                status_line,
                accept_valid,
                max_unfragmented_message_bytes: header(
                    "x-codex-websocket-max-unfragmented-message-bytes",
                ),
            },
        ))
    }

    fn send(&mut self, opcode: u8, payload: &[u8]) -> Result<(), WsError> {
        let mask: [u8; 4] = random_bytes(4)
            .try_into()
            .map_err(|_| WsError::Protocol("mask generation"))?;
        self.stream
            .write_all(&encode_frame(opcode, payload, mask))?;
        self.stream.flush()?;
        self.frames_sent += 1;
        Ok(())
    }

    pub fn send_text(&mut self, text: &str) -> Result<(), WsError> {
        self.send(0x1, text.as_bytes())
    }

    fn read_frame(&mut self) -> Result<(bool, u8, Vec<u8>), WsError> {
        let mut head = [0u8; 2];
        self.stream.read_exact(&mut head)?;
        let fin = head[0] & 0x80 != 0;
        if head[0] & 0x70 != 0 {
            return Err(WsError::Protocol("reserved bits set"));
        }
        let opcode = head[0] & 0x0F;
        if head[1] & 0x80 != 0 {
            return Err(WsError::Protocol("server frame masked"));
        }
        let length = match head[1] & 0x7F {
            126 => {
                let mut bytes = [0u8; 2];
                self.stream.read_exact(&mut bytes)?;
                u64::from(u16::from_be_bytes(bytes))
            }
            127 => {
                let mut bytes = [0u8; 8];
                self.stream.read_exact(&mut bytes)?;
                u64::from_be_bytes(bytes)
            }
            short => u64::from(short),
        };
        if opcode >= 0x8 && (length > 125 || !fin) {
            return Err(WsError::Protocol("invalid control frame"));
        }
        if length > MAX_MESSAGE as u64 {
            return Err(WsError::TooLarge(length));
        }
        let mut payload = vec![0u8; length as usize];
        self.stream.read_exact(&mut payload)?;
        Ok((fin, opcode, payload))
    }

    /// Next data or close message. Pings are answered with pongs; pongs are ignored.
    pub fn next_message(&mut self) -> Result<Message, WsError> {
        let mut assembling: Option<(u8, Vec<u8>)> = None;
        loop {
            let (fin, opcode, payload) = self.read_frame()?;
            match opcode {
                0x9 => {
                    self.send(0xA, &payload)?;
                    self.pongs_sent += 1;
                    continue;
                }
                0xA => continue,
                0x8 => {
                    let code =
                        (payload.len() >= 2).then(|| u16::from_be_bytes([payload[0], payload[1]]));
                    return Ok(Message::Close(code));
                }
                0x1 | 0x2 if assembling.is_none() => assembling = Some((opcode, payload)),
                0x0 => match assembling.as_mut() {
                    Some((_, buffer)) => {
                        if buffer.len() + payload.len() > MAX_MESSAGE {
                            return Err(WsError::TooLarge((buffer.len() + payload.len()) as u64));
                        }
                        buffer.extend_from_slice(&payload);
                    }
                    None => return Err(WsError::Protocol("continuation without start")),
                },
                _ => return Err(WsError::Protocol("unexpected opcode")),
            }
            if fin {
                return match assembling.take() {
                    Some((0x1, bytes)) => String::from_utf8(bytes)
                        .map(Message::Text)
                        .map_err(|_| WsError::Protocol("text frame not UTF-8")),
                    Some((_, bytes)) => Ok(Message::Binary(bytes.len())),
                    None => Err(WsError::Protocol("empty message")),
                };
            }
        }
    }

    /// Sends a normal-closure frame and waits (bounded by the stream's read
    /// timeout) for the peer's close.
    pub fn close(&mut self) -> Value {
        if let Err(error) = self.send(0x8, &1000u16.to_be_bytes()) {
            return json!({ "sent": false, "error": error.code() });
        }
        for _ in 0..MAX_LOGGED {
            match self.next_message() {
                Ok(Message::Close(code)) => return json!({ "sent": true, "peerCloseCode": code }),
                Ok(_) => continue,
                Err(error) => return json!({ "sent": true, "peerClose": error.code() }),
            }
        }
        json!({ "sent": true, "peerClose": "NOT_RECEIVED" })
    }
}

/// Reads messages until the response to `id`, recording (never answering)
/// server requests and notifications by method name only.
fn await_response<S: Read + Write>(
    connection: &mut Connection<S>,
    id: i64,
    deadline: Instant,
    seen: &mut Vec<Value>,
) -> Result<Value, WsError> {
    loop {
        if Instant::now() >= deadline {
            return Err(WsError::Timeout);
        }
        let text = match connection.next_message()? {
            Message::Text(text) => text,
            Message::Binary(length) => {
                if seen.len() < MAX_LOGGED {
                    seen.push(json!({ "kind": "binary", "bytes": length }));
                }
                continue;
            }
            Message::Close(code) => return Err(WsError::Closed(code)),
        };
        let Ok(Value::Object(message)) = serde_json::from_str::<Value>(&text) else {
            if seen.len() < MAX_LOGGED {
                seen.push(json!({ "kind": "unparsed", "bytes": text.len() }));
            }
            continue;
        };
        let method = message.get("method").and_then(Value::as_str);
        match (message.get("id"), method) {
            (Some(found), None) if found.as_i64() == Some(id) => return Ok(Value::Object(message)),
            (Some(_), Some(method)) => {
                if seen.len() < MAX_LOGGED {
                    seen.push(
                        json!({ "kind": "serverRequest", "method": method, "answered": false }),
                    );
                }
            }
            (None, Some(method)) => {
                if seen.len() < MAX_LOGGED {
                    seen.push(json!({ "kind": "notification", "method": method }));
                }
            }
            _ => {
                if seen.len() < MAX_LOGGED {
                    seen.push(json!({ "kind": "otherResponse" }));
                }
            }
        }
    }
}

/// The version the released daemon client extracts from `userAgent`:
/// the token after the first `/`, up to whitespace.
pub fn version_from_user_agent(user_agent: &str) -> Option<String> {
    let (_, rest) = user_agent.split_once('/')?;
    rest.split_whitespace()
        .next()
        .filter(|version| !version.is_empty())
        .map(str::to_string)
}

fn error_summary(response: &Map<String, Value>) -> Value {
    let error = response.get("error");
    json!({
        "code": error.and_then(|e| e.get("code")).cloned(),
        "message": error.and_then(|e| e.get("message")).cloned(),
    })
}

/// Runs the passive observer sequence on a connected stream. The caller sets
/// the stream's read/write timeouts; `budget` bounds each response wait.
/// Every step is recorded so a partial run still yields evidence.
pub fn observe<S: Read + Write>(
    stream: S,
    budget: Duration,
    loaded_list_limit: Option<u32>,
) -> Value {
    let mut record = Map::new();
    let started = Instant::now();
    let (mut connection, upgrade) = match Connection::client(stream) {
        Ok(pair) => pair,
        Err(error) => {
            record.insert("upgrade".into(), json!({ "error": error.code() }));
            record.insert("completed".into(), json!(false));
            return Value::Object(record);
        }
    };
    record.insert(
        "upgrade".into(),
        json!({
            "statusLine": upgrade.status_line,
            "acceptValid": upgrade.accept_valid,
            "maxUnfragmentedMessageBytes": upgrade.max_unfragmented_message_bytes,
        }),
    );

    let mut seen = Vec::new();
    let client_info = json!({
        "name": CLIENT_NAME,
        "title": CLIENT_TITLE,
        "version": env!("CARGO_PKG_VERSION"),
    });
    let initialize =
        json!({ "id": 1, "method": "initialize", "params": { "clientInfo": client_info } });
    let mut completed = false;
    let outcome = (|| -> Result<(), WsError> {
        connection.send_text(&initialize.to_string())?;
        let response = await_response(&mut connection, 1, Instant::now() + budget, &mut seen)?;
        let Value::Object(response) = response else {
            return Err(WsError::Protocol("response not an object"));
        };
        let Some(result) = response.get("result") else {
            record.insert(
                "initialize".into(),
                json!({ "sent": initialize, "error": error_summary(&response) }),
            );
            return Err(WsError::Protocol("initialize returned an error"));
        };
        let user_agent = result
            .get("userAgent")
            .and_then(Value::as_str)
            .unwrap_or_default();
        record.insert(
            "initialize".into(),
            json!({
                "sent": initialize,
                "capabilitiesAdvertised": Value::Null,
                "response": {
                    "userAgent": user_agent,
                    "appServerVersion": version_from_user_agent(user_agent),
                    "platformFamily": result.get("platformFamily"),
                    "platformOs": result.get("platformOs"),
                    "codexHome": result.get("codexHome"),
                    "fields": result.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()),
                },
            }),
        );
        connection.send_text(&json!({ "method": "initialized" }).to_string())?;
        record.insert("initializedSent".into(), json!(true));

        if let Some(limit) = loaded_list_limit {
            let request =
                json!({ "id": 2, "method": "thread/loaded/list", "params": { "limit": limit } });
            connection.send_text(&request.to_string())?;
            let response = await_response(&mut connection, 2, Instant::now() + budget, &mut seen)?;
            let entry = match response.get("result") {
                Some(result) => json!({
                    "sent": request,
                    "count": result.get("data").and_then(Value::as_array).map(Vec::len),
                    "hasNextCursor": result.get("nextCursor").is_some_and(|cursor| !cursor.is_null()),
                }),
                None => json!({
                    "sent": request,
                    "error": response.as_object().map(error_summary),
                }),
            };
            record.insert("loadedList".into(), entry);
        }
        Ok(())
    })();
    match outcome {
        Ok(()) => completed = true,
        Err(error) => {
            record.insert("error".into(), json!(error.code()));
        }
    }
    record.insert("close".into(), connection.close());
    record.insert("messagesSeenWhileWaiting".into(), Value::Array(seen));
    record.insert("framesSent".into(), json!(connection.frames_sent));
    record.insert("pongsSent".into(), json!(connection.pongs_sent));
    record.insert(
        "elapsedMs".into(),
        json!(started.elapsed().as_millis() as u64),
    );
    record.insert("completed".into(), json!(completed));
    Value::Object(record)
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::{UnixListener, UnixStream};

    use super::*;

    #[test]
    fn sha1_matches_known_vectors() {
        let hex = |bytes: [u8; 20]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(hex(sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            hex(sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn base64_pads_correctly() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn accept_matches_rfc6455_example() {
        assert_eq!(
            accept_for("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn frames_are_masked_with_correct_length_encoding() {
        let mask = [1, 2, 3, 4];
        for length in [0usize, 125, 126, 65_535, 65_536] {
            let payload = vec![0x5Au8; length];
            let frame = encode_frame(0x1, &payload, mask);
            assert_eq!(frame[0], 0x81);
            assert_eq!(frame[1] & 0x80, 0x80, "client frames must be masked");
            let (header, declared) = match frame[1] & 0x7F {
                126 => (4, u16::from_be_bytes([frame[2], frame[3]]) as usize),
                127 => (
                    10,
                    u64::from_be_bytes(frame[2..10].try_into().unwrap_or_default()) as usize,
                ),
                short => (2, short as usize),
            };
            assert_eq!(declared, length);
            assert_eq!(&frame[header..header + 4], &mask);
            let unmasked: Vec<u8> = frame[header + 4..]
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4])
                .collect();
            assert_eq!(unmasked, payload);
        }
    }

    #[test]
    fn user_agent_version_matches_released_parser() {
        assert_eq!(
            version_from_user_agent(
                "codex_app_server_daemon/1.2.3 (Linux 6.8.0; x86_64) codex_cli_rs/1.2.3"
            )
            .as_deref(),
            Some("1.2.3")
        );
        assert_eq!(version_from_user_agent("codex_app_server_daemon"), None);
    }

    // ---- In-process fake app-server: the server half of RFC 6455 ----

    fn server_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0x80 | opcode];
        if payload.len() < 126 {
            out.push(payload.len() as u8);
        } else {
            out.push(126);
            out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        out.extend_from_slice(payload);
        out
    }

    /// Reads one client frame, asserting it is masked; returns (opcode, payload).
    fn read_client_frame(stream: &mut impl Read) -> (u8, Vec<u8>) {
        let mut head = [0u8; 2];
        stream.read_exact(&mut head).expect("frame head");
        assert_eq!(head[1] & 0x80, 0x80, "client frame was not masked");
        let length = match head[1] & 0x7F {
            126 => {
                let mut bytes = [0u8; 2];
                stream.read_exact(&mut bytes).expect("length");
                usize::from(u16::from_be_bytes(bytes))
            }
            127 => panic!("unexpectedly large client frame"),
            short => usize::from(short),
        };
        let mut mask = [0u8; 4];
        stream.read_exact(&mut mask).expect("mask");
        let mut payload = vec![0u8; length];
        stream.read_exact(&mut payload).expect("payload");
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[index % 4];
        }
        (head[0] & 0x0F, payload)
    }

    fn read_client_json(stream: &mut impl Read) -> Value {
        let (opcode, payload) = read_client_frame(stream);
        assert_eq!(opcode, 0x1, "expected a text frame");
        serde_json::from_slice(&payload).expect("client JSON")
    }

    /// Plays the released server's role and returns every method the client sent.
    fn fake_server(listener: UnixListener) -> Vec<String> {
        let (stream, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let mut writer = stream;
        let mut key = None;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("header line");
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("sec-websocket-key")
            {
                key = Some(value.trim().to_string());
            }
        }
        let accept = accept_for(&key.expect("client sent a key"));
        write!(
            writer,
            "HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: {accept}\r\nx-codex-websocket-max-unfragmented-message-bytes: 1048576\r\n\r\n"
        )
        .expect("upgrade response");

        let mut methods = Vec::new();
        let initialize = read_client_json(&mut reader);
        methods.push(
            initialize["method"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        );
        assert_eq!(initialize["id"], 1);
        assert_eq!(initialize["params"]["clientInfo"]["name"], CLIENT_NAME);
        assert!(initialize["params"].get("capabilities").is_none());
        // A notification and a ping arrive before the response.
        writer
            .write_all(&server_frame(0x1, br#"{"method":"thread/status/changed","params":{"threadId":"t","status":{"type":"idle"}}}"#))
            .expect("notification");
        writer.write_all(&server_frame(0x9, b"hb")).expect("ping");
        let (opcode, payload) = read_client_frame(&mut reader);
        assert_eq!(
            (opcode, payload.as_slice()),
            (0xA, b"hb".as_slice()),
            "pong echoes ping"
        );
        // The response is split across a text frame and a continuation.
        let response = br#"{"id":1,"result":{"userAgent":"codex_app_server_daemon/9.9.9 (test) codex_cli_rs/9.9.9","codexHome":"/home/x/.codex","platformFamily":"unix","platformOs":"macos"}}"#;
        let (first, second) = response.split_at(20);
        writer
            .write_all(&[0x01, first.len() as u8])
            .expect("fragment head");
        writer.write_all(first).expect("fragment");
        writer
            .write_all(&server_frame(0x0, second))
            .expect("continuation");

        let initialized = read_client_json(&mut reader);
        methods.push(
            initialized["method"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        );
        assert!(
            initialized.get("id").is_none(),
            "initialized is a notification"
        );

        let list = read_client_json(&mut reader);
        methods.push(list["method"].as_str().unwrap_or_default().to_string());
        assert_eq!(list["params"]["limit"], 5);
        writer
            .write_all(&server_frame(
                0x1,
                br#"{"id":2,"result":{"data":["a","b"],"nextCursor":"c"}}"#,
            ))
            .expect("list response");

        let (opcode, _) = read_client_frame(&mut reader);
        assert_eq!(opcode, 0x8, "client closes");
        writer
            .write_all(&server_frame(0x8, &1000u16.to_be_bytes()))
            .expect("close reply");
        methods
    }

    #[test]
    fn observer_completes_handshake_against_fake_server() {
        let dir = std::env::temp_dir().join(format!("tsp-ws-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir(&dir).expect("temp dir");
        let path = dir.join("s");
        let listener = UnixListener::bind(&path).expect("bind");
        let server = std::thread::spawn(move || fake_server(listener));

        let stream = UnixStream::connect(&path).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        let record = observe(stream, Duration::from_secs(5), Some(5));
        let methods = server.join().expect("server thread");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(methods, ["initialize", "initialized", "thread/loaded/list"]);
        assert_eq!(record["completed"], true, "{record}");
        assert_eq!(record["upgrade"]["acceptValid"], true);
        assert_eq!(
            record["initialize"]["response"]["appServerVersion"],
            "9.9.9"
        );
        assert_eq!(record["loadedList"]["count"], 2);
        assert_eq!(record["loadedList"]["hasNextCursor"], true);
        assert_eq!(record["pongsSent"], 1);
        assert_eq!(record["close"]["peerCloseCode"], 1000);
        assert_eq!(
            record["messagesSeenWhileWaiting"][0]["method"],
            "thread/status/changed"
        );
    }

    #[test]
    fn upgrade_with_wrong_accept_is_rejected() {
        let dir = std::env::temp_dir().join(format!("tsp-ws-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir(&dir).expect("temp dir");
        let path = dir.join("s");
        let listener = UnixListener::bind(&path).expect("bind");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            let _ = stream.write_all(
                b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: wrong\r\n\r\n",
            );
        });
        let stream = UnixStream::connect(&path).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        let record = observe(stream, Duration::from_secs(5), Some(5));
        let _ = server.join();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(record["completed"], false);
        assert_eq!(
            record["upgrade"]["error"],
            "PROTOCOL:invalid upgrade headers"
        );
    }
}
