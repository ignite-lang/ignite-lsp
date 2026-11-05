//! A minimal JSON reader (RFC 8259) for `metaModel.json`: objects keep their member order,
//! numbers keep their text. The generator only reads, so there is no writer.

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(String),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// The one value of `text`, whitespace allowed around it.
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut parser = Parser {
            bytes: text.as_bytes(),
            at: 0,
        };
        parser.space();
        let value = parser.value(0)?;
        parser.space();
        if parser.at != parser.bytes.len() {
            return Err(format!("unexpected data at byte {}", parser.at));
        }
        Ok(value)
    }

    /// The first member `key` of an object.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

const MAXIMUM_DEPTH: usize = 256;

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn error<T>(&self, message: &str) -> Result<T, String> {
        Err(format!("{message} at byte {}", self.at))
    }

    fn space(&mut self) {
        while self
            .bytes
            .get(self.at)
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.at += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAXIMUM_DEPTH {
            return self.error("nested too deeply");
        }
        match self.peek() {
            None => self.error("unexpected end of input"),
            Some(b'n') => self.word("null", Json::Null),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => self.error("unexpected character"),
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            self.error("unexpected character")
        }
    }

    fn digits(&mut self) -> Result<(), String> {
        let start = self.at;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.at += 1;
        }
        if self.at == start {
            return self.error("expected a digit");
        }
        Ok(())
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        let integer = self.at;
        self.digits()?;
        if self.at - integer > 1 && self.bytes[integer] == b'0' {
            return self.error("leading zero in a number");
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            self.digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            self.digits()?;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at]).expect("ASCII digits");
        Ok(Json::Number(text.to_string()))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let Some(digits) = self.bytes.get(self.at..self.at + 4) else {
            return self.error("invalid escape");
        };
        let text = std::str::from_utf8(digits).map_err(|_| "invalid escape".to_string())?;
        let value = u32::from_str_radix(text, 16).map_err(|_| "invalid escape".to_string())?;
        self.at += 4;
        Ok(value)
    }

    fn string(&mut self) -> Result<String, String> {
        self.at += 1;
        let mut out = Vec::new();
        loop {
            match self.peek() {
                None => return self.error("unterminated string"),
                Some(b'"') => {
                    self.at += 1;
                    return String::from_utf8(out).map_err(|_| "string is not UTF-8".to_string());
                }
                Some(b'\\') => {
                    self.at += 1;
                    let Some(escape) = self.peek() else {
                        return self.error("unterminated string");
                    };
                    self.at += 1;
                    let decoded = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => self.unicode()?,
                        _ => return self.error("invalid escape"),
                    };
                    let mut buffer = [0u8; 4];
                    out.extend_from_slice(decoded.encode_utf8(&mut buffer).as_bytes());
                }
                Some(0..=0x1f) => return self.error("control character in a string"),
                Some(b) => {
                    out.push(b);
                    self.at += 1;
                }
            }
        }
    }

    fn unicode(&mut self) -> Result<char, String> {
        let first = self.hex4()?;
        let code = match first {
            0xD800..=0xDBFF => {
                if !self.bytes[self.at..].starts_with(b"\\u") {
                    return self.error("lone surrogate");
                }
                self.at += 2;
                let second = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&second) {
                    return self.error("lone surrogate");
                }
                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
            }
            0xDC00..=0xDFFF => return self.error("lone surrogate"),
            other => other,
        };
        char::from_u32(code).ok_or_else(|| "invalid escape".to_string())
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.at += 1;
        let mut items = Vec::new();
        self.space();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.space();
            items.push(self.value(depth + 1)?);
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                _ => return self.error("expected `,` or `]`"),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.at += 1;
        let mut members = Vec::new();
        self.space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Json::Object(members));
        }
        loop {
            self.space();
            if self.peek() != Some(b'"') {
                return self.error("expected a member name");
            }
            let name = self.string()?;
            self.space();
            if self.peek() != Some(b':') {
                return self.error("expected `:`");
            }
            self.at += 1;
            self.space();
            members.push((name, self.value(depth + 1)?));
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(members));
                }
                _ => return self.error("expected `,` or `}`"),
            }
        }
    }
}
