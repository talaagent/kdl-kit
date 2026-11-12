//! Standard base64 (RFC 4648, padded) for byte strings.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub(crate) fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(n >> (18 - 6 * i)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodes padded or unpadded base64; whitespace is ignored.
pub(crate) fn decode(s: &str) -> Option<Vec<u8>> {
    let digits: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let trimmed = digits
        .strip_suffix(b"==")
        .or_else(|| digits.strip_suffix(b"="))
        .unwrap_or(&digits);
    if trimmed.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(trimmed.len() * 3 / 4);
    for chunk in trimmed.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            let v = ALPHABET.iter().position(|a| a == c)?;
            n |= (v as u32) << (18 - 6 * i);
        }
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for len in 0..10 {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 200) as u8).collect();
            assert_eq!(decode(&encode(&bytes)).unwrap(), bytes);
        }
        assert_eq!(encode(b"hello"), "aGVsbG8=");
        assert_eq!(decode("aGVsbG8").unwrap(), b"hello");
        assert_eq!(decode("a"), None);
        assert_eq!(decode("a!=="), None);
    }
}
