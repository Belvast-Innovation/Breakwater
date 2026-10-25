//! Minimal strkey decoding for Ed25519 secret seeds (`S...`), so a signer
//! seed can be handed to a tool through the environment without printing it
//! or adding a dependency.

use thiserror::Error;

/// Decoding failures.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum StrkeyError {
    /// Not a 56-character base32 string.
    #[error("strkey must be 56 base32 characters")]
    Length,
    /// A character outside the base32 alphabet.
    #[error("invalid base32 character")]
    Alphabet,
    /// Not an Ed25519 secret seed (`S...`).
    #[error("not an ed25519 secret seed strkey")]
    Version,
    /// CRC16 mismatch.
    #[error("checksum mismatch")]
    Checksum,
}

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const VERSION_SEED: u8 = 18 << 3;
const VERSION_PUBLIC: u8 = 6 << 3;
const VERSION_CONTRACT: u8 = 2 << 3;

fn base32_decode(input: &str) -> Result<Vec<u8>, StrkeyError> {
    let mut out = Vec::with_capacity(input.len() * 5 / 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for c in input.bytes() {
        let v = ALPHABET
            .iter()
            .position(|&a| a == c)
            .ok_or(StrkeyError::Alphabet)? as u32;
        buffer = (buffer << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Ok(out)
}

fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn decode(strkey: &str, version: u8) -> Result<[u8; 32], StrkeyError> {
    if strkey.len() != 56 {
        return Err(StrkeyError::Length);
    }
    let bytes = base32_decode(strkey)?;
    if bytes.len() != 35 {
        return Err(StrkeyError::Length);
    }
    if bytes[0] != version {
        return Err(StrkeyError::Version);
    }
    let (payload, checksum) = bytes.split_at(33);
    let expected = crc16_xmodem(payload).to_le_bytes();
    if checksum != expected {
        return Err(StrkeyError::Checksum);
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&payload[1..33]);
    Ok(out)
}

/// Decodes an `S...` strkey into the 32-byte Ed25519 seed.
pub fn decode_seed(strkey: &str) -> Result<[u8; 32], StrkeyError> {
    decode(strkey, VERSION_SEED)
}

/// Decodes a `G...` strkey into the 32-byte Ed25519 public key.
pub fn decode_public(strkey: &str) -> Result<[u8; 32], StrkeyError> {
    decode(strkey, VERSION_PUBLIC)
}

/// Checks that `strkey` is a well-formed contract id (`C...`).
pub fn validate_contract(strkey: &str) -> Result<(), StrkeyError> {
    decode(strkey, VERSION_CONTRACT).map(|_| ())
}

/// Lower-case hex of a byte slice.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base32_encode(data: &[u8]) -> String {
        let mut out = String::new();
        let mut buffer: u32 = 0;
        let mut bits = 0;
        for &b in data {
            buffer = (buffer << 8) | u32::from(b);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
            }
        }
        if bits > 0 {
            out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
        }
        out
    }

    fn encode(version: u8, key: &[u8; 32]) -> String {
        let mut payload = vec![version];
        payload.extend_from_slice(key);
        let crc = crc16_xmodem(&payload).to_le_bytes();
        payload.extend_from_slice(&crc);
        base32_encode(&payload)
    }

    #[test]
    fn seed_and_public_keys_round_trip() {
        let key: [u8; 32] = core::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(11));
        let seed = encode(VERSION_SEED, &key);
        let public = encode(VERSION_PUBLIC, &key);
        assert_eq!(seed.len(), 56);
        assert!(seed.starts_with('S'));
        assert!(public.starts_with('G'));
        assert_eq!(decode_seed(&seed), Ok(key));
        assert_eq!(decode_public(&public), Ok(key));
        assert_eq!(decode_seed(&public), Err(StrkeyError::Version));
        assert_eq!(decode_public(&seed), Err(StrkeyError::Version));
        assert_eq!(validate_contract(&encode(VERSION_CONTRACT, &key)), Ok(()));
        assert_eq!(validate_contract(&public), Err(StrkeyError::Version));
    }

    #[test]
    fn rejects_bad_length_alphabet_and_checksum() {
        let key = [7u8; 32];
        let seed = encode(VERSION_SEED, &key);
        let mut corrupted = seed.clone();
        corrupted.replace_range(55..56, if seed.ends_with('A') { "B" } else { "A" });
        assert_eq!(decode_seed(&corrupted), Err(StrkeyError::Checksum));
        assert_eq!(decode_seed("short"), Err(StrkeyError::Length));
        let mut bad_alphabet = seed;
        bad_alphabet.replace_range(10..11, "1");
        assert_eq!(decode_seed(&bad_alphabet), Err(StrkeyError::Alphabet));
        assert_eq!(hex(&[0, 255, 16]), "00ff10");
    }
}
