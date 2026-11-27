//! Minimal GGUF metadata reader: the number of transformer blocks
//! (`<arch>.block_count`) for the GPU offload slider (ADR 0036) and the
//! trained context window (`<arch>.context_length`) for the "Max" context
//! choice (ADR 0042).
//!
//! Reads the header and key/value pairs only, never the tensors; values that
//! are not needed are skipped. Supports GGUF v2 and v3 (64-bit counts).

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek};
use std::path::Path;

/// Longest key or string value we accept (guards against corrupt files).
const MAX_STRING: u64 = 16 * 1024 * 1024;
/// Most key/value pairs we accept.
const MAX_KV: u64 = 1_000_000;

const T_U8: u32 = 0;
const T_I8: u32 = 1;
const T_U16: u32 = 2;
const T_I16: u32 = 3;
const T_U32: u32 = 4;
const T_I32: u32 = 5;
const T_F32: u32 = 6;
const T_BOOL: u32 = 7;
const T_STRING: u32 = 8;
const T_ARRAY: u32 = 9;
const T_U64: u32 = 10;
const T_I64: u32 = 11;
const T_F64: u32 = 12;

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_owned())
}

fn u32_le(r: &mut impl Read) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn u64_le(r: &mut impl Read) -> io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn string(r: &mut impl Read) -> io::Result<String> {
    let len = u64_le(r)?;
    if len > MAX_STRING {
        return Err(bad("string too long"));
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn fixed_size(t: u32) -> Option<i64> {
    match t {
        T_U8 | T_I8 | T_BOOL => Some(1),
        T_U16 | T_I16 => Some(2),
        T_U32 | T_I32 | T_F32 => Some(4),
        T_U64 | T_I64 | T_F64 => Some(8),
        _ => None,
    }
}

fn skip<R: Read + Seek>(r: &mut BufReader<R>, t: u32) -> io::Result<()> {
    if let Some(n) = fixed_size(t) {
        return r.seek_relative(n);
    }
    match t {
        T_STRING => {
            let len = u64_le(r)?;
            if len > MAX_STRING {
                return Err(bad("string too long"));
            }
            r.seek_relative(len as i64)
        }
        T_ARRAY => {
            let inner = u32_le(r)?;
            let count = u64_le(r)?;
            if let Some(n) = fixed_size(inner) {
                let bytes = count
                    .checked_mul(n as u64)
                    .ok_or_else(|| bad("array too long"))?;
                return r.seek_relative(i64::try_from(bytes).map_err(|_| bad("array too long"))?);
            }
            for _ in 0..count {
                skip(r, inner)?;
            }
            Ok(())
        }
        _ => Err(bad("unknown value type")),
    }
}

/// Unsigned integer value of type `t`, other types are skipped (`None`).
fn integer<R: Read + Seek>(r: &mut BufReader<R>, t: u32) -> io::Result<Option<u64>> {
    match t {
        T_U32 | T_I32 => Ok(Some(u64::from(u32_le(r)?))),
        T_U64 | T_I64 => Ok(Some(u64_le(r)?)),
        _ => skip(r, t).map(|()| None),
    }
}

/// Number of transformer blocks of the model in `path`, `None` when the file
/// does not say (or is GGUF v1). Errors only for unreadable or corrupt files.
pub fn block_count(path: &Path) -> io::Result<Option<u32>> {
    arch_value(path, ".block_count")
}

/// Context window the model in `path` was trained for, `None` when the file
/// does not say (or is GGUF v1). Errors only for unreadable or corrupt files.
pub fn context_length(path: &Path) -> io::Result<Option<u32>> {
    arch_value(path, ".context_length")
}

/// Integer `<general.architecture><suffix>` of the model in `path`.
fn arch_value(path: &Path, suffix: &str) -> io::Result<Option<u32>> {
    let mut r = BufReader::new(File::open(path)?);
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if &magic != b"GGUF" {
        return Err(bad("not a GGUF file"));
    }
    let version = u32_le(&mut r)?;
    if version < 2 {
        return Ok(None);
    }
    let _tensors = u64_le(&mut r)?;
    let kv_count = u64_le(&mut r)?;
    if kv_count > MAX_KV {
        return Err(bad("too many metadata entries"));
    }
    let mut arch: Option<String> = None;
    let mut values: HashMap<String, u64> = HashMap::new();
    for _ in 0..kv_count {
        let key = string(&mut r)?;
        let t = u32_le(&mut r)?;
        if key == "general.architecture" && t == T_STRING {
            arch = Some(string(&mut r)?);
        } else if let Some(prefix) = key.strip_suffix(suffix) {
            if let Some(v) = integer(&mut r, t)? {
                values.insert(prefix.to_owned(), v);
            }
        } else {
            skip(&mut r, t)?;
        }
        if let Some(a) = &arch
            && let Some(&n) = values.get(a)
        {
            return Ok(u32::try_from(n).ok());
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(out: &mut Vec<u8>, text: &str) {
        out.extend((text.len() as u64).to_le_bytes());
        out.extend(text.as_bytes());
    }

    fn header(kv: u64) -> Vec<u8> {
        let mut b = b"GGUF".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend(kv.to_le_bytes());
        b
    }

    fn write(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("m.gguf");
        std::fs::write(&path, bytes).unwrap();
        (dir, path)
    }

    #[test]
    fn reads_block_count_after_skipping_other_values() {
        let mut b = header(5);
        // general.name: string
        s(&mut b, "general.name");
        b.extend(T_STRING.to_le_bytes());
        s(&mut b, "Test");
        // tokenizer tokens: array of strings
        s(&mut b, "tokenizer.ggml.tokens");
        b.extend(T_ARRAY.to_le_bytes());
        b.extend(T_STRING.to_le_bytes());
        b.extend(2u64.to_le_bytes());
        s(&mut b, "a");
        s(&mut b, "bc");
        // another model's count must not win
        s(&mut b, "other.block_count");
        b.extend(T_U32.to_le_bytes());
        b.extend(99u32.to_le_bytes());
        s(&mut b, "qwen3.block_count");
        b.extend(T_U32.to_le_bytes());
        b.extend(36u32.to_le_bytes());
        s(&mut b, "general.architecture");
        b.extend(T_STRING.to_le_bytes());
        s(&mut b, "qwen3");
        let (_dir, path) = write(&b);
        assert_eq!(block_count(&path).unwrap(), Some(36));
        assert_eq!(context_length(&path).unwrap(), None);
    }

    #[test]
    fn reads_context_length() {
        let mut b = header(3);
        s(&mut b, "general.architecture");
        b.extend(T_STRING.to_le_bytes());
        s(&mut b, "llama");
        s(&mut b, "llama.block_count");
        b.extend(T_U32.to_le_bytes());
        b.extend(32u32.to_le_bytes());
        s(&mut b, "llama.context_length");
        b.extend(T_U64.to_le_bytes());
        b.extend(131_072u64.to_le_bytes());
        let (_dir, path) = write(&b);
        assert_eq!(context_length(&path).unwrap(), Some(131_072));
        assert_eq!(block_count(&path).unwrap(), Some(32));
    }

    #[test]
    fn missing_count_is_none_and_garbage_is_an_error() {
        let mut b = header(1);
        s(&mut b, "general.architecture");
        b.extend(T_STRING.to_le_bytes());
        s(&mut b, "llama");
        let (_dir, path) = write(&b);
        assert_eq!(block_count(&path).unwrap(), None);

        let (_dir2, junk) = write(b"NOPE1234");
        assert!(block_count(&junk).is_err());

        let mut truncated = header(1);
        s(&mut truncated, "llama.block_count");
        let (_dir3, cut) = write(&truncated);
        assert!(block_count(&cut).is_err());
    }

    /// `GGUF_PATH=<model.gguf> cargo test -p lokalsi-core --lib gguf -- --ignored --nocapture`
    #[test]
    #[ignore = "needs a real model file"]
    fn real_file() {
        let path = std::env::var("GGUF_PATH").expect("set GGUF_PATH");
        let n = block_count(Path::new(&path)).unwrap();
        let ctx = context_length(Path::new(&path)).unwrap();
        println!("block_count = {n:?}, context_length = {ctx:?}");
        assert!(n.is_some_and(|n| n > 0));
    }
}
