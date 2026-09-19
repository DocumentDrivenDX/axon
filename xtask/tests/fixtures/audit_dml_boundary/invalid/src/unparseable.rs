pub async fn broken_sql(pool: &sqlx::Pool<sqlx::Postgres>) {
    let _ = sqlx::query("INSERT INTO entities (id VALUES ($1")
        .execute(pool)
        .await;
}
