use crate::text::text_formatter::FmtValueFormatter;
use crate::Bytes;
use std::fmt::{Debug, Display, Formatter};
use std::ops::Deref;

#[derive(Copy, Clone)]
pub struct BytesRef<'data> {
    data: &'data [u8],
}

impl Deref for BytesRef<'_> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl BytesRef<'_> {
    pub fn to_owned(self) -> Bytes {
        Bytes::from(self.data)
    }

    pub fn into_owned(self) -> Bytes {
        Bytes::from(self)
    }

    pub fn data(&self) -> &[u8] {
        self.as_ref()
    }
}

impl<'data> From<BytesRef<'data>> for Bytes {
    fn from(value: BytesRef<'data>) -> Self {
        Bytes::from(value.data)
    }
}

impl<'data, const N: usize> From<&'data [u8; N]> for BytesRef<'data> {
    fn from(bytes: &'data [u8; N]) -> Self {
        BytesRef { data: bytes }
    }
}

impl<'data> From<&'data [u8]> for BytesRef<'data> {
    fn from(bytes: &'data [u8]) -> Self {
        BytesRef { data: bytes }
    }
}

impl<'data> From<&'data str> for BytesRef<'data> {
    fn from(text: &'data str) -> Self {
        BytesRef {
            data: text.as_bytes(),
        }
    }
}

impl PartialEq<[u8]> for BytesRef<'_> {
    fn eq(&self, other: &[u8]) -> bool {
        self.data() == other
    }
}

impl PartialEq<&[u8]> for BytesRef<'_> {
    fn eq(&self, other: &&[u8]) -> bool {
        self.data() == *other
    }
}

impl<'data> PartialEq<BytesRef<'data>> for [u8] {
    fn eq(&self, other: &BytesRef<'data>) -> bool {
        self == other.data()
    }
}

impl<'a> PartialEq<BytesRef<'a>> for BytesRef<'_> {
    fn eq(&self, other: &BytesRef<'a>) -> bool {
        self == other.data()
    }
}

impl Display for BytesRef<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let mut formatter = FmtValueFormatter { output: f };
        formatter
            .format_blob(self.data())
            .map_err(|_| std::fmt::Error)
    }
}

impl Debug for BytesRef<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        const NUM_BYTES_TO_SHOW: usize = 32;
        let data = self.data;
        // Shows up to the first 32 bytes in hex
        write!(f, "BytesRef: [")?;
        for byte in data.iter().copied().take(NUM_BYTES_TO_SHOW) {
            write!(f, "{byte:x} ")?;
        }
        if data.len() > NUM_BYTES_TO_SHOW {
            write!(f, "...{} more", (data.len() - NUM_BYTES_TO_SHOW))?;
        }
        write!(f, "]")?;

        Ok(())
    }
}
