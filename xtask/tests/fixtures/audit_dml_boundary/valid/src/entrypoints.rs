pub async fn insert_with_query(pool: &sqlx::Pool<sqlx::Postgres>) {
    let _ = sqlx::query("INSERT INTO entities (id, body) VALUES ($1, $2)")
        .execute(pool)
        .await;
}

pub async fn scalar_update(pool: &sqlx::Pool<sqlx::Postgres>) {
    let _ = sqlx::query_scalar::<_, i64>(
        "UPDATE entity_versions SET latest = TRUE WHERE entity_id = $1",
    )
    .fetch_one(pool)
    .await;
}

pub async fn raw_delete(pool: &sqlx::Pool<sqlx::Postgres>) {
    let _ = sqlx::raw_sql("DELETE FROM entity_tombstones WHERE expires_at < CURRENT_TIMESTAMP")
        .execute(pool)
        .await;
}

pub async fn macro_update(pool: &sqlx::Pool<sqlx::Postgres>, id: i64) {
    let _ = sqlx::query!("UPDATE schema_versions SET active = TRUE WHERE id = $1", id)
        .execute(pool)
        .await;
}

pub fn query_builder_insert() {
    let mut builder =
        sqlx::QueryBuilder::<sqlx::Postgres>::new("INSERT INTO audit_events (id) VALUES ($1)");
    builder.push(" RETURNING id");
}

pub fn sqlite_execute(conn: &rusqlite::Connection) {
    let _ = conn.execute("UPDATE sqlite_shadow SET touched = 1 WHERE id = ?1", [1_i64]);
}

pub fn sqlite_batch(conn: &rusqlite::Connection) {
    let _ = conn.execute_batch("DELETE FROM sqlite_shadow WHERE tombstoned = 1;");
}

pub async fn copy_entities(conn: &mut sqlx::PgConnection) {
    let _ = conn
        .copy_in_raw("COPY entity_imports (id, body) FROM STDIN WITH (FORMAT csv)")
        .await;
}
