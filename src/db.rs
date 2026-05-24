use anyhow::Result;
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use std::str::FromStr;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ThreadSummaryRow {
    pub snowflake: String,
    pub name: String,
    pub ai_summary: String,
    pub last_message_timestamp: i64,
    pub updated_at: i64,
    pub transcript_json: String,
}

pub async fn open_db(path: &str) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(path)?.create_if_missing(true);
    let pool = SqlitePool::connect_with(opts).await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS thread_summaries (
            snowflake                TEXT PRIMARY KEY,
            name                     TEXT NOT NULL,
            transcript_json          TEXT NOT NULL,
            ai_summary               TEXT NOT NULL,
            last_message_timestamp   INTEGER NOT NULL,
            updated_at               INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await?;
    // Clean up orphaned table left by a previous one-time migration.
    let _ = sqlx::query("DROP TABLE IF EXISTS thread_summaries_new").execute(&pool).await;
    Ok(pool)
}

pub async fn upsert_thread_summary(
    pool: &SqlitePool,
    snowflake: &str,
    name: &str,
    transcript_json: &str,
    ai_summary: &str,
    last_message_timestamp: i64,
    updated_at: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO thread_summaries
             (snowflake, name, transcript_json, ai_summary, last_message_timestamp, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(snowflake) DO UPDATE SET
             name                   = excluded.name,
             transcript_json        = excluded.transcript_json,
             ai_summary             = excluded.ai_summary,
             last_message_timestamp = excluded.last_message_timestamp,
             updated_at             = excluded.updated_at",
    )
    .bind(snowflake)
    .bind(name)
    .bind(transcript_json)
    .bind(ai_summary)
    .bind(last_message_timestamp)
    .bind(updated_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_thread_summary(pool: &SqlitePool, snowflake: &str) -> Result<Option<ThreadSummaryRow>> {
    Ok(sqlx::query_as::<_, ThreadSummaryRow>(
        "SELECT snowflake, name, ai_summary, last_message_timestamp, updated_at, transcript_json
         FROM thread_summaries WHERE snowflake = ?1 LIMIT 1",
    )
    .bind(snowflake)
    .fetch_optional(pool)
    .await?)
}

pub async fn list_thread_summaries(pool: &SqlitePool) -> Result<Vec<ThreadSummaryRow>> {
    Ok(sqlx::query_as::<_, ThreadSummaryRow>(
        "SELECT snowflake, name, ai_summary, last_message_timestamp, updated_at, transcript_json
         FROM thread_summaries ORDER BY last_message_timestamp DESC",
    )
    .fetch_all(pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use tempfile::NamedTempFile;

    async fn make_pool() -> (sqlx::SqlitePool, NamedTempFile) {
        let file = NamedTempFile::new().unwrap();
        let path = format!("sqlite:{}", file.path().display());
        let pool = super::open_db(&path).await.unwrap();
        (pool, file)
    }

    #[tokio::test]
    async fn list_empty() {
        let (pool, _f) = make_pool().await;
        assert!(super::list_thread_summaries(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn upsert_and_get_roundtrip() {
        let (pool, _f) = make_pool().await;
        super::upsert_thread_summary(&pool, "123", "my thread", "[]", "<p>hi</p>", 1000, 2000)
            .await
            .unwrap();
        let row = super::get_thread_summary(&pool, "123").await.unwrap().unwrap();
        assert_eq!(row.snowflake, "123");
        assert_eq!(row.name, "my thread");
        assert_eq!(row.transcript_json, "[]");
        assert_eq!(row.ai_summary, "<p>hi</p>");
        assert_eq!(row.last_message_timestamp, 1000);
        assert_eq!(row.updated_at, 2000);
    }

    #[tokio::test]
    async fn upsert_replaces_on_conflict() {
        let (pool, _f) = make_pool().await;
        super::upsert_thread_summary(&pool, "123", "old", "[]", "old", 1000, 2000)
            .await
            .unwrap();
        super::upsert_thread_summary(&pool, "123", "new", "[{}]", "new", 1500, 3000)
            .await
            .unwrap();
        let row = super::get_thread_summary(&pool, "123").await.unwrap().unwrap();
        assert_eq!(row.name, "new");
        assert_eq!(row.last_message_timestamp, 1500);
    }

    #[tokio::test]
    async fn list_ordered_newest_first() {
        let (pool, _f) = make_pool().await;
        super::upsert_thread_summary(&pool, "1", "older", "[]", "", 1000, 0).await.unwrap();
        super::upsert_thread_summary(&pool, "2", "newer", "[]", "", 2000, 0).await.unwrap();
        let rows = super::list_thread_summaries(&pool).await.unwrap();
        assert_eq!(rows[0].snowflake, "2");
        assert_eq!(rows[1].snowflake, "1");
    }

    #[tokio::test]
    async fn get_missing_is_none() {
        let (pool, _f) = make_pool().await;
        assert!(super::get_thread_summary(&pool, "nope").await.unwrap().is_none());
    }
}
