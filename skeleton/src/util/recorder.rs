//! Offline market-data recording and replay.
//!
//! The recorder writes the raw websocket deltas (MarketEvent) produced by
//! the exchange subscribers to a binary file, prefixed with a session header
//! carrying the per-symbol instrument metadata (tick/lot sizes, notional and
//! post-only limits) so the stream can be replayed without network access.
//!
//! Replay applies the deltas through the same LocalBook code paths the
//! live loaders use, which keeps recorded data faithful: best bid/ask
//! snapshots are tagged with their bba flag, and book snapshots include
//! their timestamps so stale events are dropped exactly as live.
//!
//! File layout: SessionHeader (bincode) followed by a stream of
//! bincode-serialized MarketEvents.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::exchanges::exchange::MarketEvent;
use crate::util::localorderbook::LocalBook;

/// Magic bytes identifying an rs_smm recording.
pub const MAGIC: [u8; 8] = *b"RSSMMREC";
/// Recording format version. v2 adds `contract_size` to `SymbolMeta`.
pub const VERSION: u32 = 2;

/// Instrument metadata for one symbol, captured when the recording starts so
/// replay never needs to query the exchange.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SymbolMeta {
    pub symbol: String,
    pub tick_size: f64,
    pub lot_size: f64,
    pub min_order_size: f64,
    pub min_notional: f64,
    pub post_only_max: f64,
    pub contract_size: f64,
}

impl SymbolMeta {
    /// Builds the metadata from the loader's instrument-populated book.
    pub fn from_book(symbol: &str, book: &LocalBook) -> Self {
        Self {
            symbol: symbol.to_string(),
            tick_size: book.tick_size,
            lot_size: book.lot_size,
            min_order_size: book.min_order_size,
            min_notional: book.min_notional,
            post_only_max: book.post_only_max,
            contract_size: book.contract_size,
        }
    }
}

/// Session header written once at the start of a recording.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SessionHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub started_at_ms: u64,
    pub exchange: String,
    pub symbols: Vec<SymbolMeta>,
}

/// Writes MarketEvent deltas to a recording file.
pub struct Recorder {
    writer: BufWriter<File>,
}

impl Recorder {
    /// Creates a recording at path and writes the session header.
    ///
    /// books supplies the instrument metadata per symbol; it should be the
    /// loader's instrument-populated books (before any market updates).
    pub fn new(
        path: impl AsRef<Path>,
        exchange: &str,
        started_at_ms: u64,
        books: &[(String, &LocalBook)],
    ) -> std::io::Result<Self> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        let header = SessionHeader {
            magic: MAGIC,
            version: VERSION,
            started_at_ms,
            exchange: exchange.to_string(),
            symbols: books
                .iter()
                .map(|(symbol, book)| SymbolMeta::from_book(symbol, book))
                .collect(),
        };
        bincode::serialize_into(&mut writer, &header).map_err(io_error)?;
        Ok(Self { writer })
    }

    /// Appends one event to the recording.
    pub fn record(&mut self, event: &MarketEvent) -> std::io::Result<()> {
        bincode::serialize_into(&mut self.writer, event).map_err(io_error)
    }

    /// Flushes pending bytes; dropping the recorder also flushes.
    pub fn finish(mut self) -> std::io::Result<()> {
        self.writer.flush()
    }

    /// Flushes buffered events to disk without closing the recording.
    /// Called periodically so a Ctrl+C shutdown loses at most the last
    /// flush interval instead of the whole buffered tail.
    pub fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

/// Reads a recording back: exposes the SessionHeader and streams the
/// recorded MarketEvents in order.
pub struct ReplayStream {
    reader: BufReader<File>,
    pub header: SessionHeader,
}

impl ReplayStream {
    /// Opens a recording and validates its header.
    ///
    /// The magic bytes and version are checked before the full header is
    /// deserialized, so arbitrary files cannot trick bincode into allocating
    /// from attacker-controlled length prefixes.
    pub fn open(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let mut magic = [0u8; 8];
        let mut version_bytes = [0u8; 4];
        reader.read_exact(&mut magic)?;
        reader.read_exact(&mut version_bytes)?;
        let version = u32::from_le_bytes(version_bytes);
        if magic != MAGIC || version != VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("not an rs_smm recording (magic {:?}, version {})", magic, version),
            ));
        }
        reader.rewind()?;
        let header: SessionHeader =
            bincode::deserialize_from(&mut reader).map_err(io_error)?;
        Ok(Self { reader, header })
    }

    /// Returns the next event, or None at end of file.
    pub fn next_event(&mut self) -> std::io::Result<Option<MarketEvent>> {
        match bincode::deserialize_from(&mut self.reader) {
            Ok(event) => Ok(Some(event)),
            Err(e) => match e.as_ref() {
                bincode::ErrorKind::Io(io) if io.kind() == std::io::ErrorKind::UnexpectedEof => {
                    Ok(None)
                }
                _ => Err(io_error(e)),
            },
        }
    }
}

fn io_error<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::helpers::generate_timestamp;
    use bybit::{Ask, Bid, TickDirection, WsTrade};

    fn sample_events() -> Vec<MarketEvent> {
        vec![
            MarketEvent::Book {
                symbol: "BTCUSDT".into(),
                bids: vec![Bid { price: 100.0, qty: 1.5 }],
                asks: vec![Ask { price: 100.5, qty: 2.0 }],
                timestamp: 1_000,
                bba: true,
            },
            MarketEvent::Trade {
                symbol: "BTCUSDT".into(),
                trade: WsTrade::new(
                    1_001,
                    "BTCUSDT",
                    "Buy",
                    0.25,
                    100.5,
                    TickDirection::PlusTick,
                    "t1",
                    false,
                ),
            },
            MarketEvent::Book {
                symbol: "BTCUSDT".into(),
                bids: vec![Bid { price: 100.25, qty: 0.5 }],
                asks: vec![Ask { price: 100.75, qty: 1.0 }],
                timestamp: 1_002,
                bba: false,
            },
        ]
    }

    #[test]
    fn roundtrip() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("rs_smm_rec_{}.bin", generate_timestamp()));

        let mut book = LocalBook::new();
        book.tick_size = 0.01;
        book.lot_size = 0.001;
        book.min_order_size = 0.001;
        book.min_notional = 5.0;
        book.post_only_max = 1000.0;

        let books = vec![("BTCUSDT".to_string(), &book)];
        let events = sample_events();

        let mut recorder = Recorder::new(&path, "bybit", 42, &books).expect("create recorder");
        for ev in &events {
            recorder.record(ev).expect("record event");
        }
        recorder.finish().expect("flush");

        let mut replay = ReplayStream::open(&path).expect("open replay");
        assert_eq!(replay.header.exchange, "bybit");
        assert_eq!(replay.header.started_at_ms, 42);
        assert_eq!(replay.header.symbols.len(), 1);
        assert_eq!(replay.header.symbols[0].tick_size, 0.01);
        assert_eq!(replay.header.symbols[0].min_notional, 5.0);

        let mut got = Vec::new();
        while let Some(ev) = replay.next_event().expect("next event") {
            got.push(ev);
        }
        assert_eq!(got.len(), events.len());
        for (a, b) in got.iter().zip(events.iter()) {
            assert_eq!(format!("{:?}", a), format!("{:?}", b));
        }

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_bad_header() {
        let dir = std::env::temp_dir();

        // Too short to hold a header.
        let path = dir.join(format!("rs_smm_bad_{}.bin", generate_timestamp()));
        std::fs::write(&path, b"junk").expect("write junk");
        assert!(ReplayStream::open(&path).is_err());
        let _ = std::fs::remove_file(&path);

        // Valid magic but an unsupported version.
        let path2 = dir.join(format!("rs_smm_bad_{}.bin", generate_timestamp()));
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&999u32.to_le_bytes());
        std::fs::write(&path2, bytes).expect("write junk");
        assert!(ReplayStream::open(&path2).is_err());
        let _ = std::fs::remove_file(&path2);
    }
}
