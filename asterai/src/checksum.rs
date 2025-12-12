use eyre::eyre;
use serde::de::Visitor;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::{Digest, Sha256};
use std::array::TryFromSliceError;
use std::fmt;
use std::fmt::{Debug, Display, Formatter};

#[derive(Clone, Eq, PartialEq, Hash, Copy)]
pub struct Checksum([u8; 32]);

struct PluginIdVisitor;

impl Checksum {
    /// Creates a new `Checksum` instance with the provided checksum (32 bytes).
    pub fn new(value: [u8; 32]) -> Self {
        Self(value)
    }

    /// Creates a new `Checksum` instance with the provided checksum byte vec.
    /// Returns an error if the vector does not hold 32 bytes.
    pub fn new_from_vec(vec: Vec<u8>) -> eyre::Result<Self> {
        let parsed = vec
            .try_into()
            .map_err(|e| eyre!("failed to parse into Checksum"))?;
        Ok(Self::new(parsed))
    }

    /// Processes the SHA256 checksum of `bytes` and returns a new `Checksum`
    /// instance holding the checksum.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut hash = Sha256::new();
        hash.update(bytes);
        let bytes = hash.finalize().to_vec();
        bytes
            .try_into()
            .expect("sha256 did not result in 256 bytes during checksum generation")
    }

    pub fn from_str(str: &str) -> Self {
        let bytes = str.as_bytes();
        Self::from_bytes(bytes)
    }

    pub fn bytes(&self) -> [u8; 32] {
        self.0.clone()
    }

    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let string = format!("0x{}", hex::encode(&self.0));
        f.write_str(&string)
    }
}

impl TryFrom<Vec<u8>> for Checksum {
    type Error = TryFromSliceError;

    fn try_from(vec: Vec<u8>) -> Result<Self, Self::Error> {
        let value = <[u8; 32]>::try_from(vec.as_slice())?;
        Ok(Self(value))
    }
}

impl Display for Checksum {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.fmt(f)
    }
}

impl Debug for Checksum {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.fmt(f)
    }
}

impl Serialize for Checksum {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let string = format!("{}", &self);
        serializer.serialize_str(&string)
    }
}

impl<'de> Visitor<'de> for PluginIdVisitor {
    type Value = Checksum;

    fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter
            .write_str("a checksum represented as a 256-bit hexadecimal string starting with 0x")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let hex_str = v
            .strip_prefix("0x")
            .ok_or_else(|| E::custom(format!("expected string to start with 0x")))?;
        let vec = hex::decode(hex_str).map_err(de::Error::custom)?;
        Checksum::try_from(vec).map_err(de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for Checksum {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_str(PluginIdVisitor)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn serialize_checksum() {
        let value: [u8; 32] = [
            0x00, 0x1a, 0x88, 0xb4, 0x0d, 0x7e, 0x4a, 0x0a, 0x02, 0x3c, 0x69, 0x10, 0xd1, 0x04,
            0x92, 0xca, 0x5f, 0x30, 0x61, 0xe0, 0xf9, 0x66, 0x38, 0x2d, 0x24, 0x4c, 0xdd, 0x1a,
            0xff, 0x87, 0x9d, 0xd2,
        ];
        let id = Checksum::new(value);
        assert_eq!(
            &id.to_string(),
            "0x001a88b40d7e4a0a023c6910d10492ca5f3061e0f966382d244cdd1aff879dd2"
        );
        let serialized = serde_json::to_string(&id).unwrap();
        assert_eq!(
            serialized,
            r#""0x001a88b40d7e4a0a023c6910d10492ca5f3061e0f966382d244cdd1aff879dd2""#
        );
    }

    #[test]
    fn deserialize_checksum() {
        let value = r#""0x001a88b40d7e4a0a023c6910d10492ca5f3061e0f966382d244cdd1aff879dd2""#;
        let id = serde_json::from_str::<Checksum>(value).unwrap();
        assert_eq!(
            &id.to_string(),
            "0x001a88b40d7e4a0a023c6910d10492ca5f3061e0f966382d244cdd1aff879dd2"
        );
    }
}
