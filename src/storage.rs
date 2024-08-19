use std::str::FromStr;

use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};

use crate::cometbft::Block;

/// The current version of the storage
const VERSION: &'static str = "penumbra-reindexer-archive-v1";

async fn create_pool() -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str("sqlite://:memory:")?;
    SqlitePool::connect_with(options).await.map_err(Into::into)
}

/// Storage used for the archive format.
pub struct Storage {
    pool: SqlitePool,
}

#[allow(dead_code)]
impl Storage {
    async fn init(&self) -> anyhow::Result<()> {
        async fn create_tables(pool: &SqlitePool) -> anyhow::Result<()> {
            sqlx::query(
                r#"CREATE TABLE IF NOT EXISTS metadata (
                    version TEXT NOT NULL UNIQUE
                );"#,
            )
            .execute(pool)
            .await?;

            // This table exists to store large blobs outside of tables.
            // This allows us to scan, e.g. for querying the max height,
            // without having to traverse the big blobs.
            sqlx::query(
                r#"CREATE TABLE IF NOT EXISTS blobs (
                    data BLOB NOT NULL
                )
                "#,
            )
            .execute(pool)
            .await?;

            sqlx::query(
                r#"CREATE TABLE IF NOT EXISTS blocks (
                    height INTEGER NOT NULL PRIMARY KEY,
                    data_id INTEGER NOT NULL
                )
                "#,
            )
            .execute(pool)
            .await?;

            // For efficient joins between blocks and the data inside.
            sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_blocks_data_id ON blocks(data_id)")
                .execute(pool)
                .await?;

            Ok(())
        }

        async fn populate_version(pool: &SqlitePool) -> anyhow::Result<()> {
            sqlx::query("INSERT OR ABORT INTO metadata (version) VALUES (?)")
                .bind(VERSION)
                .execute(pool)
                .await?;
            Ok(())
        }

        create_tables(&self.pool).await?;
        populate_version(&self.pool).await?;

        Ok(())
    }

    async fn check_version(&self) -> anyhow::Result<()> {
        let version = self.version().await?;
        anyhow::ensure!(
            version == VERSION,
            "mismatched database version: expected {}, actual {}",
            VERSION,
            version
        );
        Ok(())
    }

    /// Create a new storage instance.
    #[tracing::instrument]
    pub async fn new() -> anyhow::Result<Self> {
        let out = Self {
            pool: create_pool().await?,
        };

        out.init().await?;
        out.check_version().await?;

        Ok(out)
    }

    /// The version of the storage.
    ///
    /// Different versions will be incompatible, requiring a data migration.
    pub async fn version(&self) -> anyhow::Result<String> {
        let (out,) = sqlx::query_as("SELECT version FROM metadata")
            .fetch_one(&self.pool)
            .await?;
        Ok(out)
    }

    /// Put a block into storage.
    ///
    /// This will fail if the block already exists.
    pub async fn put_block(&self, height: u64, block: Block) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;

        let (data_id,): (i64,) =
            sqlx::query_as("INSERT INTO blobs(data) VALUES (?) RETURNING rowid")
                .bind(&block.encode())
                .fetch_one(tx.as_mut())
                .await?;
        sqlx::query("INSERT INTO blocks(height, data_id) VALUES (?, ?)")
            .bind(i64::try_from(height)?)
            .bind(data_id)
            .execute(tx.as_mut())
            .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Get a block from storage.
    ///
    /// This will return [Option::None] if there's no such block.
    pub async fn get_block(&self, height: u64) -> anyhow::Result<Option<Block>> {
        let data: Option<(Vec<u8>,)> = sqlx::query_as(
            "SELECT (data) FROM blocks JOIN blobs ON data_id = blobs.rowid WHERE height = ?",
        )
        .bind(i64::try_from(height)?)
        .fetch_optional(&self.pool)
        .await?;
        Ok(data.map(|x| Block::decode(&x.0)).transpose()?)
    }

    /// Get the highest known block in the storage.
    pub async fn last_height(&self) -> anyhow::Result<u64> {
        let (height,): (i64,) = sqlx::query_as("SELECT MAX(height) FROM blocks")
            .fetch_one(&self.pool)
            .await?;
        Ok(height.try_into()?)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_storage_can_get_version() -> anyhow::Result<()> {
        assert_eq!(Storage::new().await?.version().await?.as_str(), VERSION);
        Ok(())
    }

    #[tokio::test]
    async fn test_put_then_get_block() -> anyhow::Result<()> {
        let in_block = Block::test_value();
        let height = 1;
        let storage = Storage::new().await?;
        storage.put_block(height, in_block.clone()).await?;
        let out_block = storage.get_block(height).await?;
        assert_eq!(out_block, Some(in_block));
        let last_height = storage.last_height().await?;
        assert_eq!(last_height, height);
        Ok(())
    }

    #[tokio::test]
    async fn test_bad_height_returns_no_block() -> anyhow::Result<()> {
        let storage = Storage::new().await?;
        assert!(storage.get_block(100).await?.is_none());
        Ok(())
    }
}
