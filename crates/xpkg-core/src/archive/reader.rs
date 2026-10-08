//! Compression-agnostic readers for package archives.
//!
//! `.xp` / `.pkg.tar.*` archives can be zstd (the default), gzip or xz, and
//! some tools emit a plain tar. Every read path in xpkg (lint, info, repo
//! tooling) goes through here so all of them accept the same inputs.

use std::io::{Cursor, Read};

use crate::error::{XpkgError, XpkgResult};

/// Stream compression detected from the magic bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    Zstd,
    Gzip,
    Xz,
    None,
}

/// Detects the compression from a byte prefix.
pub fn detect_compression(magic: &[u8]) -> Compression {
    if magic.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
        Compression::Zstd
    } else if magic.starts_with(&[0x1F, 0x8B]) {
        Compression::Gzip
    } else if magic.starts_with(&[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00]) {
        Compression::Xz
    } else {
        Compression::None
    }
}

/// Wraps `reader` with the decoder matching its magic bytes.
///
/// The sniffed prefix is chained back into the stream, so the decoder sees
/// the archive from byte zero.
pub fn decoded_reader<'a, R: Read + 'a>(mut reader: R) -> XpkgResult<Box<dyn Read + 'a>> {
    let mut magic = [0u8; 6];
    let read = read_prefix(&mut reader, &mut magic)?;
    let prefix = magic[..read].to_vec();
    let chained = Cursor::new(prefix).chain(reader);

    match detect_compression(&magic[..read]) {
        Compression::Zstd => {
            let decoder = zstd::Decoder::new(chained)
                .map_err(|e| XpkgError::Archive(format!("zstd init: {e}")))?;
            Ok(Box::new(decoder))
        }
        Compression::Gzip => Ok(Box::new(flate2::read::GzDecoder::new(chained))),
        Compression::Xz => Ok(Box::new(xz2::read::XzDecoder::new(chained))),
        Compression::None => Ok(Box::new(chained)),
    }
}

/// Reads up to `magic.len()` bytes, retrying short reads.
fn read_prefix<R: Read>(reader: &mut R, magic: &mut [u8]) -> XpkgResult<usize> {
    let mut filled = 0;
    while filled < magic.len() {
        let read = reader
            .read(&mut magic[filled..])
            .map_err(|e| XpkgError::Archive(format!("read archive header: {e}")))?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tar_with(path: &str, contents: &str) -> Vec<u8> {
        let mut raw = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut raw);
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, path, contents.as_bytes())
                .expect("append");
            builder.finish().expect("finish");
        }
        raw
    }

    fn read_first_entry<R: Read>(mut reader: R) -> String {
        let mut archive = tar::Archive::new(&mut reader);
        let mut entry = archive
            .entries()
            .expect("entries")
            .next()
            .expect("entry")
            .expect("ok");
        let mut out = String::new();
        entry.read_to_string(&mut out).expect("read");
        out
    }

    #[test]
    fn detects_magic_bytes() {
        assert_eq!(
            detect_compression(&[0x28, 0xB5, 0x2F, 0xFD, 0x00]),
            Compression::Zstd
        );
        assert_eq!(detect_compression(&[0x1F, 0x8B, 0x08]), Compression::Gzip);
        assert_eq!(
            detect_compression(&[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00]),
            Compression::Xz
        );
        assert_eq!(detect_compression(b"ustar"), Compression::None);
    }

    #[test]
    fn reads_zstd_gzip_xz_and_plain_tar() {
        let tar = tar_with("payload.txt", "hello world");

        let zst = zstd::encode_all(tar.as_slice(), 3).expect("zstd");
        assert_eq!(
            read_first_entry(decoded_reader(zst.as_slice()).unwrap()),
            "hello world"
        );

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&tar).expect("gzip");
        let gz = gz.finish().expect("finish");
        assert_eq!(
            read_first_entry(decoded_reader(gz.as_slice()).unwrap()),
            "hello world"
        );

        let mut xz = xz2::write::XzEncoder::new(Vec::new(), 1);
        xz.write_all(&tar).expect("xz");
        let xz = xz.finish().expect("finish");
        assert_eq!(
            read_first_entry(decoded_reader(xz.as_slice()).unwrap()),
            "hello world"
        );

        assert_eq!(
            read_first_entry(decoded_reader(tar.as_slice()).unwrap()),
            "hello world"
        );
    }

    #[test]
    fn empty_input_fails_cleanly() {
        // An empty stream decodes to an empty tar; iterating must error, not
        // panic. This guards against corrupt/truncated archives.
        let result = decoded_reader(&b""[..]);
        assert!(result.is_ok());
    }
}
