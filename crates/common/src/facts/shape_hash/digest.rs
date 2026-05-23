use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, de};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ShapeHashDigest(String);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeHashDigestError {
    InvalidDigest { digest_hex: String },
}

impl fmt::Display for ShapeHashDigestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDigest { digest_hex } => write!(
                f,
                "shape hash digest `{digest_hex}` must be canonical 16-character lowercase hex"
            ),
        }
    }
}

impl std::error::Error for ShapeHashDigestError {}

impl ShapeHashDigest {
    pub fn new(digest_hex: impl Into<String>) -> Self {
        Self::try_new(digest_hex).unwrap_or_else(|err| panic!("{err}"))
    }

    pub fn try_new(digest_hex: impl Into<String>) -> Result<Self, ShapeHashDigestError> {
        let digest_hex = digest_hex.into();
        if is_canonical_shape_hash_digest(&digest_hex) {
            Ok(Self(digest_hex))
        } else {
            Err(ShapeHashDigestError::InvalidDigest { digest_hex })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for ShapeHashDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ShapeHashDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let digest_hex = String::deserialize(deserializer)?;
        Self::try_new(digest_hex).map_err(de::Error::custom)
    }
}

fn is_canonical_shape_hash_digest(digest_hex: &str) -> bool {
    digest_hex.len() == 16
        && digest_hex
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
