//! An append-only file of mined blocks and the indexer writes each caused.
//!
//! Reloading a chain reads the file and applies the recorded writes, so the
//! indexer module does not run again: a restart takes milliseconds however
//! long the chain is. The file names the indexer module that produced the
//! writes; a different module makes the runtime index the blocks again.
//!
//! A record that was cut short, as when the process is killed mid-write, is
//! dropped when the file is next opened.
//!
//! Layout: the 8-byte magic, the module's SHA-256, then one record per block:
//! a little-endian `u32` length followed by that many bytes holding the
//! height, the block, and the writes, each length-prefixed.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use bitcoin::consensus::{deserialize, serialize};
use bitcoin::Block;

use crate::indexer::Storage;

const MAGIC: &[u8; 8] = b"LCNODE01";
const HEADER: u64 = 8 + 32;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("cannot use the chain file {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not a Labcoat Network chain file; remove it or run `labcoat reset`")]
    NotAChain { path: PathBuf },
    #[error("the chain file {path} is damaged at block {height}; run `labcoat reset`")]
    Damaged { path: PathBuf, height: u32 },
    #[error("the chain file {path} is not in the expected state; `labcoat down` and `labcoat up` reload it")]
    Unusable { path: PathBuf },
}

/// One mined block and the indexer writes it caused.
pub struct Record {
    pub block: Block,
    pub writes: Storage,
}

/// What [`BlockLog::open`] found.
pub enum Opened {
    /// No chain file exists.
    Missing,
    /// The file was written with this indexer module: replay its writes.
    Matching(BlockLog, Vec<Record>),
    /// The file was written with another module: index its blocks again.
    DifferentModule(Vec<Block>),
}

pub struct BlockLog {
    path: PathBuf,
    file: File,
    /// The offset of each height's record; one past the end is the length.
    offsets: Vec<u64>,
    /// Set when a failed write could not be undone: the file may end in a
    /// partial record, so nothing more is written to it.
    broken: bool,
    #[cfg(test)]
    fail_writes: bool,
}

impl BlockLog {
    pub fn open(path: &Path, module: &[u8; 32]) -> Result<Opened, StoreError> {
        let io = |source| StoreError::Io {
            path: path.to_owned(),
            source,
        };
        let mut file = match OpenOptions::new().read(true).append(true).open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Opened::Missing)
            }
            Err(error) => return Err(io(error)),
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(io)?;
        if bytes.len() < HEADER as usize || &bytes[..8] != MAGIC {
            return Err(StoreError::NotAChain {
                path: path.to_owned(),
            });
        }
        let same_module = &bytes[8..40] == module;

        let mut records = Vec::new();
        let mut offsets = Vec::new();
        let mut cursor = HEADER as usize;
        while cursor < bytes.len() {
            let Some(length) = read_u32(&bytes, cursor) else {
                break;
            };
            let start = cursor + 4;
            let Some(payload) = bytes.get(start..start + length as usize) else {
                break;
            };
            let height = records.len() as u32;
            let record = decode(payload, height).ok_or_else(|| StoreError::Damaged {
                path: path.to_owned(),
                height,
            })?;
            offsets.push(cursor as u64);
            records.push(record);
            cursor = start + length as usize;
        }
        if cursor != bytes.len() {
            tracing::warn!(
                "dropping an incomplete record after block {} in {}",
                records.len().saturating_sub(1),
                path.display()
            );
            file.set_len(cursor as u64).map_err(io)?;
        }
        offsets.push(cursor as u64);

        if !same_module {
            return Ok(Opened::DifferentModule(
                records.into_iter().map(|record| record.block).collect(),
            ));
        }
        file.seek(SeekFrom::End(0)).map_err(io)?;
        Ok(Opened::Matching(
            Self {
                path: path.to_owned(),
                file,
                offsets,
                broken: false,
                #[cfg(test)]
                fail_writes: false,
            },
            records,
        ))
    }

    /// Start an empty chain file, replacing any file at `path`.
    pub fn create(path: &Path, module: &[u8; 32]) -> Result<Self, StoreError> {
        let io = |source| StoreError::Io {
            path: path.to_owned(),
            source,
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(path)
            .map_err(io)?;
        file.write_all(MAGIC).map_err(io)?;
        file.write_all(module).map_err(io)?;
        file.flush().map_err(io)?;
        drop(file);
        // Reopen for appending, so every record lands at the end of the
        // file, also after it is shortened.
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .open(path)
            .map_err(io)?;
        Ok(Self {
            path: path.to_owned(),
            file,
            offsets: vec![HEADER],
            broken: false,
            #[cfg(test)]
            fail_writes: false,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The number of blocks recorded.
    pub fn len(&self) -> usize {
        self.offsets.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Record the next block.
    pub fn append(
        &mut self,
        height: u32,
        block: &Block,
        writes: &Storage,
    ) -> Result<(), StoreError> {
        if self.broken || height as usize != self.len() {
            return Err(StoreError::Unusable {
                path: self.path.clone(),
            });
        }
        let mut payload = Vec::new();
        payload.extend_from_slice(&height.to_le_bytes());
        put(&mut payload, &serialize(block));
        payload.extend_from_slice(&(writes.len() as u32).to_le_bytes());
        for (key, value) in writes {
            put(&mut payload, key);
            put(&mut payload, value);
        }
        let mut record = Vec::with_capacity(payload.len() + 4);
        put(&mut record, &payload);

        let end = self
            .offsets
            .last()
            .copied()
            .expect("offsets start with the header");
        if let Err(source) = self.write_record(&record) {
            // Remove whatever part of the record reached the file, so the
            // file still ends at the last complete block.
            let undone = self
                .file
                .set_len(end)
                .and_then(|()| self.file.seek(SeekFrom::End(0)).map(drop));
            if undone.is_err() {
                self.broken = true;
            }
            return Err(StoreError::Io {
                path: self.path.clone(),
                source,
            });
        }
        self.offsets.push(end + record.len() as u64);
        Ok(())
    }

    /// Make later writes stop halfway and fail, as a full disk does.
    #[cfg(test)]
    pub(crate) fn fail_writes_for_test(&mut self, fail: bool) {
        self.fail_writes = fail;
    }

    fn write_record(&mut self, record: &[u8]) -> std::io::Result<()> {
        #[cfg(test)]
        if self.fail_writes {
            self.file.write_all(&record[..record.len() / 2])?;
            self.file.flush()?;
            return Err(std::io::Error::other("no space left on device"));
        }
        self.file.write_all(record)?;
        self.file.flush()
    }

    /// Keep blocks up to and including `height`.
    pub fn truncate(&mut self, height: u32) -> Result<(), StoreError> {
        let keep = height as usize + 2;
        if keep >= self.offsets.len() {
            return Ok(());
        }
        // Shorten the file first: if that fails, the offsets still describe
        // it and the next append continues where it was.
        let length = self.offsets[keep - 1];
        self.set_len(length).map_err(|source| StoreError::Io {
            path: self.path.clone(),
            source,
        })?;
        self.offsets.truncate(keep);
        Ok(())
    }

    fn set_len(&mut self, length: u64) -> std::io::Result<()> {
        #[cfg(test)]
        if self.fail_writes {
            return Err(std::io::Error::other("no space left on device"));
        }
        // The file is opened for appending, so writes go to its end
        // wherever the cursor is.
        self.file.set_len(length)
    }
}

fn put(buffer: &mut Vec<u8>, bytes: &[u8]) {
    buffer.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buffer.extend_from_slice(bytes);
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn take<'a>(bytes: &'a [u8], cursor: &mut usize) -> Option<&'a [u8]> {
    let length = read_u32(bytes, *cursor)? as usize;
    let start = *cursor + 4;
    let slice = bytes.get(start..start + length)?;
    *cursor = start + length;
    Some(slice)
}

fn decode(payload: &[u8], expected_height: u32) -> Option<Record> {
    if read_u32(payload, 0)? != expected_height {
        return None;
    }
    let mut cursor = 4;
    let block: Block = deserialize(take(payload, &mut cursor)?).ok()?;
    let count = read_u32(payload, cursor)?;
    cursor += 4;
    let mut writes = Storage::new();
    for _ in 0..count {
        let key = take(payload, &mut cursor)?.to_vec();
        let value = take(payload, &mut cursor)?.to_vec();
        writes.insert(key, value);
    }
    (cursor == payload.len()).then_some(Record { block, writes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::Chain;
    use bitcoin::{Amount, ScriptBuf};

    fn temporary(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("labcoat-store-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory.join("chain.log")
    }

    fn chain(blocks: u32) -> Chain {
        let mut chain = Chain::regtest();
        for _ in 0..blocks {
            chain.mine(
                Vec::new(),
                ScriptBuf::from_bytes(vec![0x51]),
                Amount::ZERO,
                0,
            );
        }
        chain
    }

    fn writes(height: u32) -> Storage {
        Storage::from([(b"tip".to_vec(), height.to_le_bytes().to_vec())])
    }

    fn matching(opened: Opened) -> (BlockLog, Vec<Record>) {
        match opened {
            Opened::Matching(log, records) => (log, records),
            _ => panic!("expected a matching chain file"),
        }
    }

    #[test]
    fn records_and_reloads_blocks_with_their_writes() {
        let path = temporary("reload");
        let chain = chain(2);
        assert!(matches!(
            BlockLog::open(&path, &[1; 32]).unwrap(),
            Opened::Missing
        ));

        let mut log = BlockLog::create(&path, &[1; 32]).unwrap();
        for height in 0..=2 {
            log.append(height, chain.block(height).unwrap(), &writes(height))
                .unwrap();
        }
        drop(log);

        let (log, records) = matching(BlockLog::open(&path, &[1; 32]).unwrap());
        assert_eq!(log.len(), 3);
        assert_eq!(records.len(), 3);
        assert_eq!(records[2].block.block_hash(), chain.block_hash(2).unwrap());
        assert_eq!(records[2].writes, writes(2));

        match BlockLog::open(&path, &[2; 32]).unwrap() {
            Opened::DifferentModule(blocks) => assert_eq!(blocks.len(), 3),
            _ => panic!("a different module must not replay these writes"),
        }
    }

    #[test]
    fn drops_a_record_cut_short_and_truncates_on_request() {
        let path = temporary("torn");
        let chain = chain(2);
        let mut log = BlockLog::create(&path, &[1; 32]).unwrap();
        for height in 0..=2 {
            log.append(height, chain.block(height).unwrap(), &writes(height))
                .unwrap();
        }
        drop(log);
        let length = std::fs::metadata(&path).unwrap().len();
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        file.set_len(length - 5).unwrap();
        drop(file);

        let (mut log, records) = matching(BlockLog::open(&path, &[1; 32]).unwrap());
        assert_eq!(records.len(), 2);
        log.append(2, chain.block(2).unwrap(), &writes(2)).unwrap();
        log.truncate(0).unwrap();
        drop(log);

        let (log, records) = matching(BlockLog::open(&path, &[1; 32]).unwrap());
        assert_eq!((log.len(), records.len()), (1, 1));
    }

    #[test]
    fn refuses_a_file_that_is_not_a_chain() {
        let path = temporary("foreign");
        std::fs::write(&path, b"something else entirely, and long enough").unwrap();
        assert!(matches!(
            BlockLog::open(&path, &[1; 32]),
            Err(StoreError::NotAChain { .. })
        ));
    }
}
