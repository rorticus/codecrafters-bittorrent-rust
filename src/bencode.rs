use serde_json;

use base64::{Engine, engine::general_purpose};

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

pub fn decode_bencoded_value(encoded_value: &[u8]) -> Result<serde_json::Value, BencodeError> {
    let mut pos = 0;
    return parse_value(encoded_value, &mut pos);
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

                if *pos > encoded_value.len() {
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
    let byte_slice = &encoded_value[*pos..*pos + string_length];
    let str = match std::str::from_utf8(byte_slice) {
        Ok(s) => s.to_string(),
        Err(_) => format!(
            "data://base64,{}",
            general_purpose::STANDARD.encode(byte_slice),
        ),
    };
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
        if encoded_value[*pos] == b'e' {
            break;
        }

        let key = parse_value(encoded_value, pos)?;
        let value = parse_value(encoded_value, pos)?;

        if !key.is_string() {
            return Err(BencodeError::InvalidDictionaryKey(key.to_string()));
        }

        entries.insert(key.as_str().unwrap().to_string(), value);
    }

    // consume the e
    *pos += 1;

    return Ok(serde_json::Value::Object(entries));
}

pub fn bencode_value(value: &serde_json::Value) -> Vec<u8> {
    if value.is_array() {
        return bencode_list(value);
    } else if value.is_object() {
        return bencode_dictionary(value);
    } else if value.is_string() {
        return bencode_string(value);
    } else if value.is_i64() {
        return bencode_integer(value);
    }

    return Vec::new();
}

fn bencode_list(value: &serde_json::Value) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::new();

    bytes.push(b'l');
    for value in value.as_array().unwrap() {
        bytes.extend(bencode_value(value));
    }
    bytes.push(b'e');

    return bytes;
}

fn bencode_dictionary(value: &serde_json::Value) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::new();

    bytes.push(b'd');

    let map = value.as_object().unwrap();
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();

    for key in keys {
        let key_val = serde_json::Value::String(key.to_string());
        bytes.extend(bencode_value(&key_val));
        bytes.extend(bencode_value(&map.get(key).unwrap()));
    }

    bytes.push(b'e');

    return bytes;
}

fn bencode_string(value: &serde_json::Value) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::new();

    let str = value.as_str().unwrap();

    if str.starts_with("data://base64,") {
        let decoded = general_purpose::STANDARD.decode(&str[14..]).unwrap();
        bytes.extend(format!("{}", decoded.len()).as_bytes());
        bytes.push(b':');
        bytes.extend(decoded);
    } else {
        bytes.extend(format!("{}", str.len()).as_bytes());
        bytes.push(b':');
        bytes.extend(str.as_bytes());
    }

    return bytes;
}

fn bencode_integer(value: &serde_json::Value) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::new();

    bytes.push(b'i');
    bytes.extend(format!("{}", value.as_i64().unwrap()).as_bytes());
    bytes.push(b'e');

    return bytes;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_integer() {
        let value = "i54e".as_bytes();
        let result = decode_bencoded_value(&value).unwrap();
        assert!(result.is_i64());
        assert_eq!(result.as_i64().unwrap(), 54);
    }

    #[test]
    fn test_decode_negative_integer() {
        let value = "i-54e".as_bytes();
        let result = decode_bencoded_value(&value).unwrap();
        assert!(result.is_i64());
        assert_eq!(result.as_i64().unwrap(), -54);
    }

    #[test]
    fn test_decode_string() {
        let value = "5:hello".as_bytes();
        let result = decode_bencoded_value(&value).unwrap();
        assert!(result.is_string());
        assert_eq!(result.as_str().unwrap(), "hello");
    }

    #[test]
    fn test_decode_list() {
        let value = "l5:helloi16ee".as_bytes();
        let result = decode_bencoded_value(&value).unwrap();

        assert!(result.is_array());
        assert_eq!(result.as_array().unwrap()[0].as_str().unwrap(), "hello");
        assert_eq!(result.as_array().unwrap()[1].as_i64().unwrap(), 16);
    }

    #[test]
    fn test_decode_list_2() {
        let value = "lli4eei5ee".as_bytes();
        let result = decode_bencoded_value(&value).unwrap();

        assert!(result.is_array());
        assert_eq!(result.as_array().unwrap().len(), 2);
        assert!(result.as_array().unwrap()[0].is_array());
        assert_eq!(result.as_array().unwrap()[1].as_i64().unwrap(), 5);
    }

    #[test]
    fn test_decode_dictionary() {
        let value = "d3:foo3:bar5:helloi52ee".as_bytes();
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

    #[test]
    fn test_decode_binary() {
        let torrent_bytes: Vec<u8> = Vec::from([
            100, 56, 58, 97, 110, 110, 111, 117, 110, 99, 101, 53, 53, 58, 104, 116, 116, 112, 58,
            47, 47, 98, 105, 116, 116, 111, 114, 114, 101, 110, 116, 45, 116, 101, 115, 116, 45,
            116, 114, 97, 99, 107, 101, 114, 46, 99, 111, 100, 101, 99, 114, 97, 102, 116, 101,
            114, 115, 46, 105, 111, 47, 97, 110, 110, 111, 117, 110, 99, 101, 49, 48, 58, 99, 114,
            101, 97, 116, 101, 100, 32, 98, 121, 49, 51, 58, 109, 107, 116, 111, 114, 114, 101,
            110, 116, 32, 49, 46, 49, 52, 58, 105, 110, 102, 111, 100, 54, 58, 108, 101, 110, 103,
            116, 104, 105, 50, 57, 57, 52, 49, 50, 48, 101, 52, 58, 110, 97, 109, 101, 49, 50, 58,
            99, 111, 100, 101, 114, 99, 97, 116, 46, 103, 105, 102, 49, 50, 58, 112, 105, 101, 99,
            101, 32, 108, 101, 110, 103, 116, 104, 105, 50, 54, 50, 49, 52, 52, 101, 54, 58, 112,
            105, 101, 99, 101, 115, 50, 52, 48, 58, 60, 52, 48, 159, 174, 191, 1, 228, 156, 15, 99,
            201, 11, 126, 220, 194, 37, 155, 106, 208, 184, 81, 155, 46, 169, 187, 55, 63, 245,
            103, 246, 68, 66, 129, 86, 201, 138, 29, 0, 252, 157, 200, 19, 102, 88, 117, 54, 244,
            140, 32, 152, 161, 215, 150, 146, 242, 89, 15, 217, 166, 3, 60, 97, 231, 23, 248, 192,
            209, 229, 88, 80, 104, 14, 180, 81, 227, 84, 59, 98, 3, 111, 84, 231, 70, 236, 54, 159,
            101, 243, 45, 69, 247, 123, 31, 28, 55, 98, 31, 185, 101, 198, 86, 112, 75, 120, 16,
            126, 213, 83, 189, 8, 19, 249, 47, 239, 120, 2, 103, 192, 123, 116, 49, 184, 104, 49,
            55, 210, 15, 245, 148, 177, 241, 191, 63, 136, 53, 22, 93, 104, 251, 4, 50, 189, 142,
            119, 150, 8, 210, 119, 130, 183, 121, 199, 115, 128, 98, 233, 181, 10, 181, 214, 188,
            4, 9, 160, 243, 169, 80, 56, 87, 102, 157, 71, 254, 117, 45, 69, 119, 234, 0, 168, 110,
            230, 171, 188, 48, 205, 219, 128, 10, 11, 98, 215, 162, 150, 17, 17, 102, 216, 57, 120,
            63, 82, 183, 15, 12, 144, 45, 86, 25, 107, 211, 238, 127, 55, 155, 93, 181, 126, 59,
            61, 141, 185, 227, 77, 182, 59, 75, 161, 190, 39, 147, 9, 17, 170, 55, 179, 249, 151,
            221, 101, 101,
        ]);

        let result = decode_bencoded_value(&torrent_bytes).unwrap();

        assert!(result.is_object());
    }

    #[test]
    fn test_encode_string() {
        let value = serde_json::Value::String("hello".to_string());
        let encoded = bencode_string(&value);

        assert_eq!(str::from_utf8(&encoded).unwrap(), "5:hello");
    }

    #[test]
    fn test_encode_integer() {
        let value = serde_json::Value::Number(serde_json::Number::from(-54));
        let encoded = bencode_integer(&value);

        assert_eq!(str::from_utf8(&encoded).unwrap(), "i-54e");
    }

    #[test]
    fn test_encode_list() {
        let value = serde_json::Value::Array(vec![
            serde_json::Value::Number(serde_json::Number::from(12)),
            serde_json::Value::String("hello".to_string()),
        ]);

        let encoded = bencode_list(&value);
        assert_eq!(str::from_utf8(&encoded).unwrap(), "li12e5:helloe");
    }

    #[test]
    fn test_encode_dictionary() {
        let mut map = serde_json::Map::new();

        map.insert(
            "b".to_string(),
            serde_json::Value::String("hello b".to_string()),
        );

        map.insert(
            "a".to_string(),
            serde_json::Value::String("hello a".to_string()),
        );

        let value = serde_json::Value::Object(map);
        let encoded = bencode_dictionary(&value);

        assert_eq!(
            str::from_utf8(&encoded).unwrap(),
            "d1:a7:hello a1:b7:hello be"
        );
    }

    #[test]
    fn test_encode_binary() {
        let torrent_bytes: Vec<u8> = Vec::from([
            100, 56, 58, 97, 110, 110, 111, 117, 110, 99, 101, 53, 53, 58, 104, 116, 116, 112, 58,
            47, 47, 98, 105, 116, 116, 111, 114, 114, 101, 110, 116, 45, 116, 101, 115, 116, 45,
            116, 114, 97, 99, 107, 101, 114, 46, 99, 111, 100, 101, 99, 114, 97, 102, 116, 101,
            114, 115, 46, 105, 111, 47, 97, 110, 110, 111, 117, 110, 99, 101, 49, 48, 58, 99, 114,
            101, 97, 116, 101, 100, 32, 98, 121, 49, 51, 58, 109, 107, 116, 111, 114, 114, 101,
            110, 116, 32, 49, 46, 49, 52, 58, 105, 110, 102, 111, 100, 54, 58, 108, 101, 110, 103,
            116, 104, 105, 50, 57, 57, 52, 49, 50, 48, 101, 52, 58, 110, 97, 109, 101, 49, 50, 58,
            99, 111, 100, 101, 114, 99, 97, 116, 46, 103, 105, 102, 49, 50, 58, 112, 105, 101, 99,
            101, 32, 108, 101, 110, 103, 116, 104, 105, 50, 54, 50, 49, 52, 52, 101, 54, 58, 112,
            105, 101, 99, 101, 115, 50, 52, 48, 58, 60, 52, 48, 159, 174, 191, 1, 228, 156, 15, 99,
            201, 11, 126, 220, 194, 37, 155, 106, 208, 184, 81, 155, 46, 169, 187, 55, 63, 245,
            103, 246, 68, 66, 129, 86, 201, 138, 29, 0, 252, 157, 200, 19, 102, 88, 117, 54, 244,
            140, 32, 152, 161, 215, 150, 146, 242, 89, 15, 217, 166, 3, 60, 97, 231, 23, 248, 192,
            209, 229, 88, 80, 104, 14, 180, 81, 227, 84, 59, 98, 3, 111, 84, 231, 70, 236, 54, 159,
            101, 243, 45, 69, 247, 123, 31, 28, 55, 98, 31, 185, 101, 198, 86, 112, 75, 120, 16,
            126, 213, 83, 189, 8, 19, 249, 47, 239, 120, 2, 103, 192, 123, 116, 49, 184, 104, 49,
            55, 210, 15, 245, 148, 177, 241, 191, 63, 136, 53, 22, 93, 104, 251, 4, 50, 189, 142,
            119, 150, 8, 210, 119, 130, 183, 121, 199, 115, 128, 98, 233, 181, 10, 181, 214, 188,
            4, 9, 160, 243, 169, 80, 56, 87, 102, 157, 71, 254, 117, 45, 69, 119, 234, 0, 168, 110,
            230, 171, 188, 48, 205, 219, 128, 10, 11, 98, 215, 162, 150, 17, 17, 102, 216, 57, 120,
            63, 82, 183, 15, 12, 144, 45, 86, 25, 107, 211, 238, 127, 55, 155, 93, 181, 126, 59,
            61, 141, 185, 227, 77, 182, 59, 75, 161, 190, 39, 147, 9, 17, 170, 55, 179, 249, 151,
            221, 101, 101,
        ]);

        let decoded = decode_bencoded_value(&torrent_bytes).unwrap();
        let encoded = bencode_value(&decoded);

        assert_eq!(torrent_bytes.len(), encoded.len());
    }
}
