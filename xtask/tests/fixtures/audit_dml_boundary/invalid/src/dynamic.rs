pub async fn dynamic_table(pool: &sqlx::Pool<sqlx::Postgres>, table_name: &str) {
    let sql = format!("INSERT INTO {table_name} (id) VALUES ($1)");
    let _ = sqlx::query(&sql).execute(pool).await;
}
