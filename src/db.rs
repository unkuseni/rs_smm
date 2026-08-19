//! Turso (libSQL) telemetry database integration.
//!
//! When [turso] url is configured, the market maker periodically flushes
//! three kinds of rows to the database:
//!
//! - feature_snapshots: one row per symbol per sync tick (mid, skew,
//!   volatility, imbalance, OFI, trade imbalance, VOI, regime, prediction),
//! - fills: every matched grid fill and market-order rebalance, including
//!   the adverse-selection ratio where a roundtrip could be paired,
//! - grid_events: grid refreshes and in-place amendments.
//!
//! The same schema works against a remote Turso database (libsql:// URL +
//! auth token) or a local file for tests/analysis.

use crate::trader::quote_gen::FillRecord;

/// One engine snapshot destined for the feature_snapshots table.
#[derive(Debug, Clone)]
pub struct FeatureSnapshot {
    pub mid: f64,
    pub skew: f64,
    pub vol: f64,
    pub imb: f64,
    pub ofi_scaled: f64,
    pub trade_imb: f64,
    pub voi: f64,
    pub regime: i8,
    pub predicted: f64,
}

/// A connection to a Turso/libSQL database.
pub struct TursoDb {
    conn: libsql::Connection,
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS feature_snapshots (
    ts INTEGER NOT NULL,
    symbol TEXT NOT NULL,
    mid REAL,
    skew REAL,
    vol REAL,
    imb REAL,
    ofi_scaled REAL,
    trade_imb REAL,
    voi REAL,
    regime INTEGER,
    predicted REAL,
    PRIMARY KEY (ts, symbol)
);
CREATE TABLE IF NOT EXISTS fills (
    ts INTEGER NOT NULL,
    symbol TEXT NOT NULL,
    side TEXT NOT NULL,
    qty REAL NOT NULL,
    price REAL NOT NULL,
    maker INTEGER NOT NULL,
    adverse_ratio REAL
);
CREATE INDEX IF NOT EXISTS idx_fills_symbol_ts ON fills (symbol, ts);
CREATE TABLE IF NOT EXISTS grid_events (
    ts INTEGER NOT NULL,
    symbol TEXT NOT NULL,
    kind TEXT NOT NULL,
    levels INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_grid_symbol_ts ON grid_events (symbol, ts);
";

/// Escapes a string literal for raw SQL (single quotes doubled).
fn sql_text(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Formats a float for raw SQL; non-finite values become NULL.
fn sql_real(v: f64) -> String {
    if v.is_finite() {
        format!("{}", v)
    } else {
        "NULL".to_string()
    }
}

fn sql_opt_real(v: Option<f64>) -> String {
    v.map(sql_real).unwrap_or_else(|| "NULL".to_string())
}

impl TursoDb {
    /// Connects to a remote Turso database over libSQL/Hrana.
    pub async fn connect(url: &str, token: &str) -> Result<Self, String> {
        let db = libsql::Builder::new_remote(url.to_string(), token.to_string())
            .build()
            .await
            .map_err(|e| e.to_string())?;
        let conn = db.connect().map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }

    /// Connects to a local libSQL file (tests and offline analysis).
    pub async fn connect_local(path: &str) -> Result<Self, String> {
        let db = libsql::Builder::new_local(path)
            .build()
            .await
            .map_err(|e| e.to_string())?;
        let conn = db.connect().map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }

    /// Creates the telemetry schema (idempotent).
    pub async fn init(&self) -> Result<(), String> {
        self.conn
            .execute_batch(SCHEMA)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Executes a batch of raw SQL statements in one roundtrip.
    pub async fn execute_batch_raw(&self, sql: String) -> Result<(), String> {
        if sql.is_empty() {
            return Ok(());
        }
        self.conn
            .execute_batch(&sql)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Builds the INSERT statement for one feature snapshot. Duplicate
    /// (ts, symbol) keys are ignored so a replayed flush is idempotent.
    pub fn feature_insert_sql(ts: u64, symbol: &str, s: &FeatureSnapshot) -> String {
        format!(
            "INSERT INTO feature_snapshots (ts, symbol, mid, skew, vol, imb, ofi_scaled, \
             trade_imb, voi, regime, predicted) VALUES ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}) \
             ON CONFLICT(ts, symbol) DO NOTHING;",
            ts,
            sql_text(symbol),
            sql_real(s.mid),
            sql_real(s.skew),
            sql_real(s.vol),
            sql_real(s.imb),
            sql_real(s.ofi_scaled),
            sql_real(s.trade_imb),
            sql_real(s.voi),
            s.regime,
            sql_real(s.predicted),
        )
    }

    /// Builds the INSERT statement for one fill record.
    pub fn fill_insert_sql(symbol: &str, f: &FillRecord) -> String {
        format!(
            "INSERT INTO fills (ts, symbol, side, qty, price, maker, adverse_ratio) \
             VALUES ({}, {}, {}, {}, {}, {}, {});",
            f.ts,
            sql_text(symbol),
            sql_text(&f.side),
            sql_real(f.qty),
            sql_real(f.price),
            i64::from(f.maker),
            sql_opt_real(f.adverse_ratio),
        )
    }

    /// Builds the INSERT statement for one grid event.
    pub fn grid_insert_sql(ts: u64, symbol: &str, kind: &str, levels: i64) -> String {
        format!(
            "INSERT INTO grid_events (ts, symbol, kind, levels) VALUES ({}, {}, {}, {});",
            ts,
            sql_text(symbol),
            sql_text(kind),
            levels,
        )
    }

    /// Returns the row count of a table (tests and diagnostics).
    pub async fn count_rows(&self, table: &str) -> Result<i64, String> {
        let mut rows = self
            .conn
            .query(&format!("SELECT COUNT(*) FROM {}", table), ())
            .await
            .map_err(|e| e.to_string())?;
        let row = rows
            .next()
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no count row".to_string())?;
        row.get::<i64>(0).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_db(name: &str) -> TursoDb {
        let path = std::env::temp_dir().join(format!("rs_smm_turso_{}.db", name));
        let _ = std::fs::remove_file(&path);
        let db = TursoDb::connect_local(path.to_str().expect("path"))
            .await
            .expect("connect local");
        db.init().await.expect("init schema");
        db
    }

    #[tokio::test]
    async fn telemetry_roundtrip() {
        let db = test_db("roundtrip").await;
        let snap = FeatureSnapshot {
            mid: 100.25,
            skew: 0.3,
            vol: 1e-4,
            imb: 0.5,
            ofi_scaled: 0.2,
            trade_imb: -0.1,
            voi: 1.5,
            regime: 1,
            predicted: 100.4,
        };
        let mut batch = TursoDb::feature_insert_sql(1000, "BTCUSDT", &snap);
        batch.push_str(&TursoDb::fill_insert_sql(
            "BTCUSDT",
            &FillRecord {
                ts: 1001,
                side: "Buy".to_string(),
                qty: 1.0,
                price: 100.0,
                maker: true,
                adverse_ratio: None,
            },
        ));
        batch.push_str(&TursoDb::grid_insert_sql(1002, "BTCUSDT", "refresh", 4));
        db.execute_batch_raw(batch).await.expect("batch insert");

        assert_eq!(db.count_rows("feature_snapshots").await.expect("count"), 1);
        assert_eq!(db.count_rows("fills").await.expect("count"), 1);
        assert_eq!(db.count_rows("grid_events").await.expect("count"), 1);

        // Duplicate snapshot key is ignored.
        db.execute_batch_raw(TursoDb::feature_insert_sql(1000, "BTCUSDT", &snap))
            .await
            .expect("dup insert");
        assert_eq!(db.count_rows("feature_snapshots").await.expect("count"), 1);
    }

    #[tokio::test]
    async fn batch_handles_non_finite_and_quotes() {
        let db = test_db("escaping").await;
        let snap = FeatureSnapshot {
            mid: f64::NAN,
            skew: f64::INFINITY,
            vol: 0.0,
            imb: 0.0,
            ofi_scaled: 0.0,
            trade_imb: 0.0,
            voi: 0.0,
            regime: 0,
            predicted: 0.0,
        };
        let sql = TursoDb::feature_insert_sql(1, "BTCUSDT", &snap);
        db.execute_batch_raw(sql).await.expect("non-finite batch");
        assert_eq!(db.count_rows("feature_snapshots").await.expect("count"), 1);

        let with_quote = TursoDb::fill_insert_sql(
            "O'NEILL",
            &FillRecord {
                ts: 2,
                side: "Sell".to_string(),
                qty: 1.0,
                price: 2.0,
                maker: false,
                adverse_ratio: Some(0.5),
            },
        );
        db.execute_batch_raw(with_quote).await.expect("quoted batch");
        assert_eq!(db.count_rows("fills").await.expect("count"), 1);
    }
}
