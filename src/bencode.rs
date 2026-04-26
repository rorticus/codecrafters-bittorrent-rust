use serde_json;

#[derive(Debug)]
pub enum BencodeError {
    UnexpectedByte(u8),
    UnexpectedEnd,
    InvalidInteger(String),
    InvalidStringLength(String),
    InvalidDictionaryKey(String),
}

impl std::fmt::Display for BencodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            BencodeError::UnexpectedByte(b) => write!(f, "Unexpected byte: {}", b),
            BencodeError::UnexpectedEnd => write!(f, "Unexpected end of input"),
            BencodeError::InvalidInteger(s) => write!(f, "Invalid integer: {}", s),
            BencodeError::InvalidStringLength(s) => write!(f, "Invalid string length: {}", s),
            BencodeError::InvalidDictionaryKey(s) => write!(f, "Invalid dictionary key: {}", s),
        }
    }
}

impl std::error::Error for BencodeError {}

pub fn decode_bencoded_value(encoded_value: &str) -> Result<serde_json::Value, BencodeError> {
    let mut pos = 0;
    return parse_value(encoded_value.as_bytes(), &mut pos);
}

fn parse_value(encoded_value: &[u8], pos: &mut usize) -> Result<serde_json::Value, BencodeError> {
    match encoded_value[*pos] {
        b'i' => parse_integer(encoded_value, pos),
        b'0'..=b'9' => parse_string(encoded_value, pos),
        b'l' => parse_list(encoded_value, pos),
        b'd' => parse_dictionary(encoded_value, pos),
        _ => Err(BencodeError::UnexpectedByte(encoded_value[*pos])),
    }
}

fn parse_integer(encoded_value: &[u8], pos: &mut usize) -> Result<serde_json::Value, BencodeError> {
    // consume the i
    *pos += 1;

    let mut string = String::new();

    // consume digits until we get to an e
    while *pos < encoded_value.len() {
        let c = encoded_value[*pos];

        match c {
            b'-' | b'0'..=b'9' => {
                string.push(c as char);
                *pos += 1;
            }
            b'e' => {
                break;
            }
            _ => return Err(BencodeError::UnexpectedByte(c)),
        }
    }

    if encoded_value[*pos] != b'e' {
        return Err(BencodeError::UnexpectedEnd);
    }

    *pos += 1;

    let number: i64 = string
        .parse()
        .map_err(|_| BencodeError::InvalidInteger(string))?;

    return Ok(serde_json::Value::Number(serde_json::Number::from(number)));
}

fn parse_string(encoded_value: &[u8], pos: &mut usize) -> Result<serde_json::Value, BencodeError> {
    let mut num_str = String::new();

    while encoded_value[*pos] != b':' {
        let c = encoded_value[*pos];

        match c {
            b'0'..=b'9' => {
                num_str.push(c as char);
                *pos += 1;

                if *pos >= encoded_value.len() {
                    return Err(BencodeError::UnexpectedEnd);
                }
            }
            b':' => {
                break;
            }
            _ => return Err(BencodeError::UnexpectedByte(c)),
        }
    }

    // consume the colon
    *pos += 1;

    let string_length: usize = num_str
        .parse()
        .map_err(|_| BencodeError::InvalidStringLength(num_str))?;

    if *pos + string_length > encoded_value.len() {
        return Err(BencodeError::UnexpectedEnd);
    }

    // read the str
    let str = std::str::from_utf8(&encoded_value[*pos..*pos + string_length])
        .map_err(|_| BencodeError::UnexpectedEnd)?
        .to_string();
    *pos += string_length;

    return Ok(serde_json::Value::String(str));
}

fn parse_list(encoded_value: &[u8], pos: &mut usize) -> Result<serde_json::Value, BencodeError> {
    // consome 'l'
    *pos += 1;
    let mut values: Vec<serde_json::Value> = Vec::new();

    loop {
        match encoded_value[*pos] {
            b'e' => break,
            _ => {
                let value = parse_value(encoded_value, pos)?;
                values.push(value);
            }
        }
    }

    //consome the e
    *pos += 1;

    return Ok(serde_json::Value::Array(values));
}

fn parse_dictionary(
    encoded_value: &[u8],
    pos: &mut usize,
) -> Result<serde_json::Value, BencodeError> {
    let mut entries: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();

    // consume the d
    *pos += 1;

    loop {
        let key = parse_value(encoded_value, pos)?;
        let value = parse_value(encoded_value, pos)?;

        if !key.is_string() {
            return Err(BencodeError::InvalidDictionaryKey(key.to_string()));
        }

        entries.insert(key.as_str().unwrap().to_string(), value);

        if encoded_value[*pos] == b'e' {
            break;
        }
    }

    // consume the e
    *pos += 1;

    return Ok(serde_json::Value::Object(entries));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_integer() {
        let value = "i54e";
        let result = decode_bencoded_value(&value).unwrap();
        assert!(result.is_i64());
        assert_eq!(result.as_i64().unwrap(), 54);
    }

    #[test]
    fn test_decode_negative_integer() {
        let value = "i-54e";
        let result = decode_bencoded_value(&value).unwrap();
        assert!(result.is_i64());
        assert_eq!(result.as_i64().unwrap(), -54);
    }

    #[test]
    fn test_decode_string() {
        let value = "5:hello";
        let result = decode_bencoded_value(&value).unwrap();
        assert!(result.is_string());
        assert_eq!(result.as_str().unwrap(), "hello");
    }

    #[test]
    fn test_decode_list() {
        let value = "l5:helloi16ee";
        let result = decode_bencoded_value(&value).unwrap();

        assert!(result.is_array());
        assert_eq!(result.as_array().unwrap()[0].as_str().unwrap(), "hello");
        assert_eq!(result.as_array().unwrap()[1].as_i64().unwrap(), 16);
    }

    #[test]
    fn test_decode_list_2() {
        let value = "lli4eei5ee";
        let result = decode_bencoded_value(&value).unwrap();

        assert!(result.is_array());
        assert_eq!(result.as_array().unwrap().len(), 2);
        assert!(result.as_array().unwrap()[0].is_array());
        assert_eq!(result.as_array().unwrap()[1].as_i64().unwrap(), 5);
    }

    #[test]
    fn test_decode_dictionary() {
        let value = "d3:foo3:bar5:helloi52ee";
        let result = decode_bencoded_value(&value).unwrap();

        assert!(result.is_object());
        assert_eq!(result.as_object().unwrap().keys().len(), 2);
        assert_eq!(
            result
                .as_object()
                .unwrap()
                .get("foo")
                .unwrap()
                .as_str()
                .unwrap(),
            "bar"
        );

        assert_eq!(
            result
                .as_object()
                .unwrap()
                .get("hello")
                .unwrap()
                .as_i64()
                .unwrap(),
            52
        );
    }
}
