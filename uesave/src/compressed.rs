//! Reading UE's chunked compressed archives: the zlib format `FArchiveLoadCompressedProxy`
//! and `FCompression` write, which some games use to wrap their saves.
//!
//! The data is one or more blocks, each a header followed by a chunk table and the zlib
//! streams of its chunks:
//!
//! ```text
//! u64 tag (0x9E2A83C1), u64 block size, u64 total compressed, u64 total uncompressed
//! (u64 compressed size, u64 uncompressed size) per chunk
//! zlib stream per chunk
//! ```

use std::io::{self, Read, Seek, SeekFrom};

const PACKAGE_FILE_TAG: u64 = 0x9E2A83C1;

struct Chunk {
    /// Offset of the chunk's first byte in the decompressed data
    start: u64,
    /// Decompressed size
    len: u64,
    /// Range of the zlib stream in the compressed data
    data: std::ops::Range<usize>,
}

/// A seekable reader over the decompressed contents of a UE compressed archive.
///
/// Chunks are only inflated when a read lands in them, so seeking past data (as
/// [`SaveReader::only`](crate::SaveReader::only) does when skipping properties) skips its
/// decompression too. Pass it to [`SaveReader::read_seekable`](crate::SaveReader::read_seekable).
pub struct CompressedReader<'a> {
    data: &'a [u8],
    chunks: Vec<Chunk>,
    /// The window of the decompressed data this reader exposes, set by [`Self::window`]
    start: u64,
    end: u64,
    /// Position relative to `start`
    position: u64,
    inflated: Option<(usize, Vec<u8>)>,
}

impl<'a> CompressedReader<'a> {
    /// Index the chunks of `data`. Nothing is inflated yet.
    pub fn new(data: &'a [u8]) -> io::Result<Self> {
        let mut chunks = vec![];
        let mut offset = 0;
        let mut start = 0;
        while offset < data.len() {
            let [tag, block_size, total_compressed, total_uncompressed] = read_u64s(data, offset)?;
            if tag != PACKAGE_FILE_TAG {
                return Err(invalid(format!(
                    "bad compressed block tag {tag:#x} at offset {offset}"
                )));
            }
            if block_size == 0 {
                return Err(invalid(format!("zero block size at offset {offset}")));
            }
            let count = total_uncompressed.div_ceil(block_size).max(1) as usize;
            let mut table = offset + 32;
            offset = table + count * 16;
            let (mut seen_compressed, mut seen_uncompressed) = (0, 0);
            for _ in 0..count {
                let [compressed, uncompressed] = read_u64s(data, table)?;
                table += 16;
                let end = offset + compressed as usize;
                if end > data.len() {
                    return Err(invalid(format!(
                        "chunk at offset {offset} runs past the data"
                    )));
                }
                chunks.push(Chunk {
                    start,
                    len: uncompressed,
                    data: offset..end,
                });
                offset = end;
                start += uncompressed;
                seen_compressed += compressed;
                seen_uncompressed += uncompressed;
            }
            if seen_compressed != total_compressed || seen_uncompressed != total_uncompressed {
                return Err(invalid(format!(
                    "block totals mismatch: compressed {seen_compressed}/{total_compressed}, \
                     uncompressed {seen_uncompressed}/{total_uncompressed}"
                )));
            }
        }
        Ok(Self {
            data,
            chunks,
            start: 0,
            end: start,
            position: 0,
            inflated: None,
        })
    }

    /// Size of the decompressed data (or of the window)
    pub fn len(&self) -> u64 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Expose only `len` bytes of the decompressed data from `start`, with positions
    /// relative to `start`. For example, to read a save written as a `TArray<uint8>`, skip
    /// its 4-byte length prefix.
    pub fn window(mut self, start: u64, len: u64) -> io::Result<Self> {
        let total = self.end;
        if start.checked_add(len).is_none_or(|end| end > total) {
            return Err(invalid(format!(
                "window {start}+{len} is past the decompressed size {total}"
            )));
        }
        self.start = start;
        self.end = start + len;
        self.position = 0;
        Ok(self)
    }

    /// Inflate everything in the window
    pub fn decompress_all(mut self) -> io::Result<Vec<u8>> {
        let mut out = Vec::with_capacity(self.len() as usize);
        self.read_to_end(&mut out)?;
        Ok(out)
    }

    fn chunk(&mut self, index: usize) -> io::Result<&[u8]> {
        if self.inflated.as_ref().is_none_or(|(i, _)| *i != index) {
            let chunk = &self.chunks[index];
            let bytes = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(
                &self.data[chunk.data.clone()],
                chunk.len as usize,
            )
            .map_err(|e| invalid(format!("chunk {index} failed to inflate: {e:?}")))?;
            if bytes.len() as u64 != chunk.len {
                return Err(invalid(format!(
                    "chunk {index} inflated to {} bytes, expected {}",
                    bytes.len(),
                    chunk.len
                )));
            }
            self.inflated = Some((index, bytes));
        }
        Ok(&self.inflated.as_ref().unwrap().1)
    }
}

impl Read for CompressedReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let absolute = self.start + self.position;
        if buf.is_empty() || absolute >= self.end {
            return Ok(0);
        }
        let index = self
            .chunks
            .partition_point(|chunk| chunk.start + chunk.len <= absolute);
        let chunk_start = self.chunks[index].start;
        let end = self.end;
        let bytes = self.chunk(index)?;
        let from = (absolute - chunk_start) as usize;
        let available = (bytes.len() - from).min((end - absolute) as usize);
        let n = buf.len().min(available);
        buf[..n].copy_from_slice(&bytes[from..from + n]);
        self.position += n as u64;
        Ok(n)
    }
}

impl Seek for CompressedReader<'_> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
            SeekFrom::End(offset) => self.len().checked_add_signed(offset),
        };
        self.position = target.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "seek to a negative position")
        })?;
        Ok(self.position)
    }
}

fn read_u64s<const N: usize>(data: &[u8], offset: usize) -> io::Result<[u64; N]> {
    let bytes = data
        .get(offset..offset + N * 8)
        .ok_or_else(|| invalid(format!("compressed data truncated at offset {offset}")))?;
    Ok(std::array::from_fn(|i| {
        u64::from_le_bytes(bytes[i * 8..i * 8 + 8].try_into().unwrap())
    }))
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
