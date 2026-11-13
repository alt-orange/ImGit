//! MD5 for Gravatar's avatar keys (nothing security related).

use ::md5::{Digest, Md5};
use std::fmt::Write as _;

/// MD5 (RFC 1321) of `data` as 32 lowercase hex digits: Gravatar's avatar key (of the trimmed, lower-case
/// e-mail address).
#[must_use]
pub fn md5_hex(data: impl AsRef<[u8]>) -> String {
    Md5::digest(data)
        .iter()
        .fold(String::with_capacity(32), |mut hex, byte| {
            // Writing to a String cannot fail.
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

#[cfg(test)]
mod tests {
    use super::md5_hex;

    /// RFC 1321's test suite.
    #[test]
    fn rfc_1321_vectors() {
        assert_eq!(md5_hex(""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex("a"), "0cc175b9c0f1b6a831c399e269772661");
        assert_eq!(md5_hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            md5_hex("message digest"),
            "f96b697d7cb7938d525a2f31aaf161d0"
        );
        assert_eq!(
            md5_hex("abcdefghijklmnopqrstuvwxyz"),
            "c3fcd3d76192e4007dfb496cca67e13b"
        );
        assert_eq!(
            md5_hex(
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890"
            ),
            "57edf4a22be3c955ac49da2e2107b67a"
        );
        assert_eq!(md5_hex(b"abc".as_slice()), md5_hex(String::from("abc")));
    }
}
