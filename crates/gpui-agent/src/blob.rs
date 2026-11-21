//! Bytes a response is too small to carry (a screenshot), handed to the
//! client in chunks that each fit one NDJSON line.
//!
//! The host keeps a blob until its last chunk is read, and at most
//! [`MAX_BLOBS`] at once: an older one is dropped when a new one arrives,
//! so a client that never reads cannot grow the host's memory.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

/// Raw bytes per chunk: 512 KiB is ~683 KiB of base64, under the 1 MiB line.
pub const BLOB_CHUNK_BYTES: usize = 512 * 1024;
/// Blobs held at once; the oldest is dropped past this.
pub const MAX_BLOBS: usize = 4;

static BLOBS: Mutex<VecDeque<(String, Vec<u8>)>> = Mutex::new(VecDeque::new());
static NEXT: AtomicU64 = AtomicU64::new(1);

fn chunk_count(len: usize) -> usize {
    len.div_ceil(BLOB_CHUNK_BYTES).max(1)
}

/// Keep `bytes` for the client and return the `result.blob` announcement.
pub fn store(bytes: Vec<u8>) -> serde_json::Value {
    // The name is the only thing a reader needs, and one process can
    // serve several hosts, so it must not be guessable from another
    // blob's name: 128 bits from the OS-seeded std hasher keys.
    use std::hash::{BuildHasher, Hasher};
    let word = || {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(NEXT.fetch_add(1, Ordering::Relaxed));
        hasher.finish()
    };
    let name = format!("b{:016x}{:016x}", word(), word());
    let announce = serde_json::json!({
        "blob": name,
        "bytes": bytes.len(),
        "chunks": chunk_count(bytes.len()),
    });
    let mut blobs = BLOBS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    while blobs.len() >= MAX_BLOBS {
        blobs.pop_front();
    }
    blobs.push_back((name, bytes));
    announce
}

/// One chunk as `{blob, index, last, data}`; the blob is dropped once its
/// last chunk has been read.
///
/// # Errors
///
/// `blob_gone` for an unknown or fully-read name, or an index past the
/// chunk count.
pub fn read_chunk(name: &str, index: u32) -> Result<serde_json::Value, String> {
    let mut blobs = BLOBS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let at = blobs
        .iter()
        .position(|(n, _)| n == name)
        .ok_or_else(|| format!("blob_gone: `{name}` is not held by this host"))?;
    let bytes = &blobs[at].1;
    let chunks = chunk_count(bytes.len());
    let index = usize::try_from(index).unwrap_or(usize::MAX);
    if index >= chunks {
        return Err(format!(
            "blob `{name}` has {chunks} chunks, not {}",
            index + 1
        ));
    }
    let start = index * BLOB_CHUNK_BYTES;
    let end = (start + BLOB_CHUNK_BYTES).min(bytes.len());
    let last = index + 1 == chunks;
    let value = serde_json::json!({
        "blob": name,
        "index": index,
        "last": last,
        "data": STANDARD.encode(&bytes[start..end]),
    });
    if last {
        blobs.remove(at);
    }
    Ok(value)
}

/// The bytes of one chunk `result` (`data` is base64).
///
/// # Errors
///
/// `"blob chunk has no data"` or invalid base64.
pub fn decode_chunk(result: &serde_json::Value) -> Result<Vec<u8>, String> {
    let data = result
        .get("data")
        .and_then(serde_json::Value::as_str)
        .ok_or("blob chunk has no data")?;
    STANDARD
        .decode(data)
        .map_err(|err| format!("blob chunk is not base64: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_reassemble_and_blob_is_dropped_after_last() {
        let bytes: Vec<u8> = (0..BLOB_CHUNK_BYTES * 2 + 17)
            .map(|i| u8::try_from(i % 256).unwrap_or_default())
            .collect();
        let announce = store(bytes.clone());
        assert_eq!(announce["chunks"], 3);
        let name = announce["blob"].as_str().unwrap();
        let mut got = Vec::new();
        for index in 0..3 {
            let chunk = read_chunk(name, index).unwrap();
            assert_eq!(chunk["last"], index == 2);
            let line = serde_json::to_string(&chunk).unwrap();
            assert!(line.len() < crate::MAX_LINE_BYTES - 4096);
            got.extend(decode_chunk(&chunk).unwrap());
        }
        assert_eq!(got, bytes);
        assert!(read_chunk(name, 0).unwrap_err().starts_with("blob_gone"));
    }

    #[test]
    fn out_of_range_chunk_is_an_error_and_keeps_the_blob() {
        let announce = store(vec![1, 2, 3]);
        let name = announce["blob"].as_str().unwrap();
        assert!(read_chunk(name, 5).is_err());
        assert_eq!(
            decode_chunk(&read_chunk(name, 0).unwrap()).unwrap(),
            [1, 2, 3]
        );
    }
}
