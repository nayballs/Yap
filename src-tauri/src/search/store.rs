//! **The vector store**: `<data>/search-index/vectors.bin`, every passage's
//! meaning as the embedding model read it, by the passage's key
//! (`chunks::key`: a hash of the model and the words). No text: the
//! passages themselves are cut from the notes as they are now whenever a
//! search runs, so nothing here goes stale or outlives a note's words.
//!
//! The file: a header (`YAPVEC01`, the dimensions, the model's tag), then one
//! record per passage — its 16-byte key, a scale (f32) and its vector as
//! signed bytes (`round(v / scale)`, scale = the largest |v| / 127): 784
//! bytes for 768 dimensions, a quarter of f32, cosines within ~0.01 of
//! exact. New records are appended (a crash mid-write loses only the record
//! it was writing: a short tail is cut off when the file is read); records
//! of passages that are gone are dropped by [`Store::compact`], which writes
//! a new file and swaps it in. A file for another model, or one that doesn't
//! read, is set aside (`vectors.bin.old`) and the index starts again.
//!
//! Brute-force search: tens of thousands of passages are a few ms.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub type Key = [u8; 16];

const MAGIC: &[u8; 8] = b"YAPVEC01";
pub const FILE: &str = "vectors.bin";

/// The vectors in memory, and where they live on disk.
pub struct Store {
    path: PathBuf,
    pub model: String,
    pub dims: usize,
    keys: HashMap<Key, usize>,
    /// Each record's key, in file order.
    order: Vec<Key>,
    scales: Vec<f32>,
    rows: Vec<i8>,
}

/// One vector as stored.
fn quantize(v: &[f32], dims: usize) -> (f32, Vec<i8>) {
    let max = v.iter().take(dims).fold(0.0f32, |m, x| m.max(x.abs()));
    let scale = if max > 0.0 { max / 127.0 } else { 1.0 };
    let mut q: Vec<i8> = v.iter().take(dims).map(|x| (x / scale).round().clamp(-127.0, 127.0) as i8).collect();
    q.resize(dims, 0);
    (scale, q)
}

fn header(model: &str, dims: usize) -> Vec<u8> {
    let mut h = Vec::with_capacity(16 + model.len());
    h.extend_from_slice(MAGIC);
    h.extend_from_slice(&(dims as u32).to_le_bytes());
    h.extend_from_slice(&(model.len() as u16).to_le_bytes());
    h.extend_from_slice(model.as_bytes());
    h
}

fn record_len(dims: usize) -> usize {
    16 + 4 + dims
}

impl Store {
    /// The store in `dir` for `model` (`dims` dimensions): what's on disk,
    /// or a new one (a file for another model, or unreadable, is set aside).
    pub fn open(dir: &Path, model: &str, dims: usize) -> Store {
        let path = dir.join(FILE);
        let mut s = Store { path: path.clone(), model: model.to_string(), dims, keys: HashMap::new(), order: Vec::new(), scales: Vec::new(), rows: Vec::new() };
        let Ok(mut f) = std::fs::File::open(&path) else { return s };
        let mut bytes = Vec::new();
        if f.read_to_end(&mut bytes).is_err() {
            return s;
        }
        drop(f);
        let want = header(model, dims);
        if !bytes.starts_with(&want) {
            if bytes.starts_with(MAGIC) {
                tracing::info!("Meeting search: the index was made by another model; starting again");
            } else {
                tracing::warn!("Meeting search: the index didn't read; starting again");
            }
            let _ = std::fs::rename(&path, path.with_extension("bin.old"));
            return s;
        }
        let rec = record_len(dims);
        let body = &bytes[want.len()..];
        let whole = body.len() / rec;
        for r in 0..whole {
            let at = &body[r * rec..(r + 1) * rec];
            let mut key = [0u8; 16];
            key.copy_from_slice(&at[..16]);
            let scale = f32::from_le_bytes([at[16], at[17], at[18], at[19]]);
            if !scale.is_finite() {
                continue;
            }
            s.put(key, scale, at[20..].iter().map(|b| *b as i8));
        }
        if body.len() % rec != 0 {
            // A record cut off by a crash: the whole ones stay.
            tracing::info!("Meeting search: a record cut off at the end of the index was dropped");
            let keep = want.len() + whole * rec;
            if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&path) {
                let _ = f.set_len(keep as u64);
            }
        }
        s
    }

    fn put(&mut self, key: Key, scale: f32, row: impl Iterator<Item = i8>) {
        if let Some(&i) = self.keys.get(&key) {
            // The same passage again (two Yaps wrote it): the newer wins.
            self.scales[i] = scale;
            for (d, v) in self.rows[i * self.dims..(i + 1) * self.dims].iter_mut().zip(row) {
                *d = v;
            }
            return;
        }
        self.keys.insert(key, self.order.len());
        self.order.push(key);
        self.scales.push(scale);
        let before = self.rows.len();
        self.rows.extend(row.take(self.dims));
        self.rows.resize(before + self.dims, 0);
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn has(&self, key: &Key) -> bool {
        self.keys.contains_key(key)
    }

    /// Add vectors (each normalised first) and append them to the file.
    pub fn add(&mut self, items: &[(Key, Vec<f32>)]) -> std::io::Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let new = !self.path.is_file();
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.path)?;
        let mut buf = Vec::with_capacity(items.len() * record_len(self.dims) + 64);
        if new || f.metadata()?.len() == 0 {
            buf.extend_from_slice(&header(&self.model, self.dims));
        }
        for (key, v) in items {
            let (scale, q) = quantize(&normalised(v), self.dims);
            buf.extend_from_slice(key);
            buf.extend_from_slice(&scale.to_le_bytes());
            buf.extend(q.iter().map(|b| *b as u8));
            self.put(*key, scale, q.into_iter());
        }
        f.write_all(&buf)?;
        f.flush()
    }

    /// The cosine of `query` (normalised) with the stored vector of `key`.
    pub fn similarity(&self, key: &Key, query: &[f32]) -> Option<f32> {
        let &i = self.keys.get(key)?;
        let row = &self.rows[i * self.dims..(i + 1) * self.dims];
        let dot: f32 = row.iter().zip(query).map(|(a, b)| *a as f32 * b).sum();
        Some(dot * self.scales[i])
    }

    /// Records whose key isn't in `keep`, as a share of all of them.
    pub fn dead_share(&self, keep: &HashSet<Key>) -> f32 {
        if self.order.is_empty() {
            return 0.0;
        }
        self.order.iter().filter(|k| !keep.contains(*k)).count() as f32 / self.order.len() as f32
    }

    /// Keep only `keep`'s records: a new file, swapped in. Returns how many
    /// went.
    pub fn compact(&mut self, keep: &HashSet<Key>) -> std::io::Result<usize> {
        let mut kept = Store { path: self.path.clone(), model: self.model.clone(), dims: self.dims, keys: HashMap::new(), order: Vec::new(), scales: Vec::new(), rows: Vec::new() };
        let mut buf = header(&self.model, self.dims);
        for (i, key) in self.order.iter().enumerate() {
            if !keep.contains(key) {
                continue;
            }
            let row = &self.rows[i * self.dims..(i + 1) * self.dims];
            buf.extend_from_slice(key);
            buf.extend_from_slice(&self.scales[i].to_le_bytes());
            buf.extend(row.iter().map(|b| *b as u8));
            kept.put(*key, self.scales[i], row.iter().copied());
        }
        let gone = self.len() - kept.len();
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("bin.tmp");
        std::fs::write(&tmp, &buf)?;
        std::fs::rename(&tmp, &self.path)?;
        *self = kept;
        Ok(gone)
    }
}

/// `v` scaled to length 1 (as is, when it's all zeros).
pub fn normalised(v: &[f32]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter().map(|x| x / n).collect()
    } else {
        v.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-search-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn k(n: u8) -> Key {
        [n; 16]
    }

    #[test]
    fn vectors_are_kept_and_read_back_close_to_exact() {
        let dir = scratch("roundtrip");
        let mut s = Store::open(&dir, "model-a", 4);
        assert_eq!(s.len(), 0);
        s.add(&[(k(1), vec![3.0, 4.0, 0.0, 0.0]), (k(2), vec![0.0, 0.0, 1.0, -1.0])]).unwrap();
        assert_eq!(s.len(), 2);
        let q = normalised(&[3.0, 4.0, 0.0, 0.0]);
        assert!((s.similarity(&k(1), &q).unwrap() - 1.0).abs() < 0.01);
        assert!(s.similarity(&k(2), &q).unwrap().abs() < 0.01);
        assert_eq!(s.similarity(&k(9), &q), None);
        // Read back from disk: the same.
        let again = Store::open(&dir, "model-a", 4);
        assert_eq!(again.len(), 2);
        assert!((again.similarity(&k(1), &q).unwrap() - 1.0).abs() < 0.01);
        assert_eq!(std::fs::metadata(dir.join(FILE)).unwrap().len(), (header("model-a", 4).len() + 2 * record_len(4)) as u64);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_crash_mid_record_loses_only_that_record() {
        let dir = scratch("torn");
        let mut s = Store::open(&dir, "m", 4);
        s.add(&[(k(1), vec![1.0, 0.0, 0.0, 0.0]), (k(2), vec![0.0, 1.0, 0.0, 0.0])]).unwrap();
        let path = dir.join(FILE);
        let len = std::fs::metadata(&path).unwrap().len();
        std::fs::OpenOptions::new().write(true).open(&path).unwrap().set_len(len - 3).unwrap();
        let s = Store::open(&dir, "m", 4);
        assert_eq!(s.len(), 1);
        assert!(s.has(&k(1)) && !s.has(&k(2)));
        // The torn tail is cut, so the next append lines up.
        let mut s = s;
        s.add(&[(k(3), vec![0.0, 0.0, 1.0, 0.0])]).unwrap();
        let s = Store::open(&dir, "m", 4);
        assert!(s.has(&k(1)) && s.has(&k(3)) && s.len() == 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn another_models_index_or_garbage_starts_again() {
        let dir = scratch("other");
        Store::open(&dir, "old-model", 4).add(&[(k(1), vec![1.0, 0.0, 0.0, 0.0])]).unwrap();
        let s = Store::open(&dir, "new-model", 4);
        assert_eq!(s.len(), 0);
        assert!(dir.join("vectors.bin.old").is_file(), "set aside, not lost");
        std::fs::write(dir.join(FILE), b"not an index").unwrap();
        assert_eq!(Store::open(&dir, "new-model", 4).len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compacting_drops_what_is_gone() {
        let dir = scratch("compact");
        let mut s = Store::open(&dir, "m", 4);
        let items: Vec<(Key, Vec<f32>)> = (1..=4).map(|n| (k(n), vec![n as f32, 1.0, 0.0, 0.0])).collect();
        s.add(&items).unwrap();
        let keep: HashSet<Key> = [k(2), k(4)].into_iter().collect();
        assert!((s.dead_share(&keep) - 0.5).abs() < 1e-6);
        assert_eq!(s.compact(&keep).unwrap(), 2);
        assert_eq!(s.len(), 2);
        let back = Store::open(&dir, "m", 4);
        assert!(back.has(&k(2)) && back.has(&k(4)) && !back.has(&k(1)));
        let q = normalised(&[4.0, 1.0, 0.0, 0.0]);
        assert!((back.similarity(&k(4), &q).unwrap() - 1.0).abs() < 0.01);
        assert!(!dir.join("vectors.bin.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_same_passage_twice_is_one_record_in_memory() {
        let dir = scratch("twice");
        let mut s = Store::open(&dir, "m", 2);
        s.add(&[(k(1), vec![1.0, 0.0])]).unwrap();
        s.add(&[(k(1), vec![0.0, 1.0])]).unwrap();
        assert_eq!(s.len(), 1);
        assert!((s.similarity(&k(1), &[0.0, 1.0]).unwrap() - 1.0).abs() < 0.01, "the newer wins");
        assert_eq!(Store::open(&dir, "m", 2).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
