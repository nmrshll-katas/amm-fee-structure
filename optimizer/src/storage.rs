//! Turso + Toasty storage for strategy results.

use anyhow::{Context, Result};
use toasty::Db;

/// Database representation of a strategy result.
#[derive(toasty::Model)]
#[table = "strategy_results"]
pub struct StoredStrategyResult {
    #[key]
    #[auto]
    pub id: i64,
    /// Template name (e.g., "boost_decay_2").
    pub template_name: String,
    /// Parameter string (e.g., "0.1_0.2_0.3").
    pub params: String,
    /// Edge value (profitability metric).
    pub edge: f64,
}

/// Storage for strategy results using Turso and Toasty.
pub struct StrategyStorage {
    db: Db,
}

impl StrategyStorage {
    /// Creates a new storage instance and ensures the database schema is ready.
    pub async fn new(database_url: &str) -> Result<Self> {
        let db = toasty::Db::builder()
            .models(toasty::models!(StoredStrategyResult))
            .connect(database_url)
            .await
            .context("Failed to open Turso database")?;
        // Run migration(s)
        let mut storage = Self { db };
        storage.migrate().await?;
        Ok(storage)
    }

    /// Ensures the `strategy_results` table exists.
    async fn migrate(&mut self) -> Result<()> {
        toasty::sql::statement(
            r#"
              CREATE TABLE IF NOT EXISTS strategy_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                template_name TEXT NOT NULL,
                params TEXT NOT NULL,
                edge REAL NOT NULL
              )
            "#,
        )
        .exec(&mut self.db)
        .await
        .context("Failed to create strategy results table")?;
        Ok(())
    }

    /// Stores a single strategy result.
    pub async fn store_result(&mut self, result: &crate::StrategyResult) -> Result<()> {
        StoredStrategyResult::create()
            .template_name(&result.template_name)
            .params(&result.params)
            .edge(result.edge)
            .exec(&mut self.db)
            .await
            .context("Failed to insert strategy result")?;
        Ok(())
    }

    /// Loads strategy results, optionally filtered by template name.
    pub async fn load_results(
        &mut self,
        template_name: Option<&str>,
    ) -> Result<Vec<crate::StrategyResult>> {
        let mut query = StoredStrategyResult::all();
        if let Some(name) = template_name {
            query = query.filter(StoredStrategyResult::fields().template_name().eq(name));
        }
        let stored = query
            .exec(&mut self.db)
            .await
            .context("Failed to load strategy results")?;

        let results = stored
            .into_iter()
            .map(|s| crate::StrategyResult {
                template_name: s.template_name,
                params: s.params,
                edge: s.edge,
            })
            .collect();
        Ok(results)
    }

    /// Checks if a specific parameter combination already exists.
    /// Useful to avoid re-running existing strategies.
    pub async fn exists(&mut self, template: &str, params: &str) -> Result<bool> {
        let count = StoredStrategyResult::all()
            .filter(StoredStrategyResult::fields().template_name().eq(template))
            .filter(StoredStrategyResult::fields().params().eq(params))
            .count()
            .exec(&mut self.db)
            .await?;
        Ok(count > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StrategyResult;

    async fn create_in_memory_storage() -> StrategyStorage {
        StrategyStorage::new("turso::memory:")
            .await
            .expect("Failed to create in‑memory storage")
    }

    #[tokio::test]
    async fn test_store_and_load_all() {
        let mut storage = create_in_memory_storage().await;

        let result1 = StrategyResult {
            template_name: "boost_decay_2".to_string(),
            params: "0.1_0.2_0.3".to_string(),
            edge: 0.123,
        };
        let result2 = StrategyResult {
            template_name: "boost_decay_example".to_string(),
            params: "0.5_0.6".to_string(),
            edge: 0.456,
        };

        storage.store_result(&result1).await.unwrap();
        storage.store_result(&result2).await.unwrap();

        let all = storage.load_results(None).await.unwrap();
        assert_eq!(all.len(), 2);
        // Order is not guaranteed, so check both exist.
        let first = &all[0];
        let second = &all[1];
        assert!(
            (first.template_name == result1.template_name
                && first.params == result1.params
                && first.edge == result1.edge)
                || (first.template_name == result2.template_name
                    && first.params == result2.params
                    && first.edge == result2.edge)
        );
        assert!(
            (second.template_name == result1.template_name
                && second.params == result1.params
                && second.edge == result1.edge)
                || (second.template_name == result2.template_name
                    && second.params == result2.params
                    && second.edge == result2.edge)
        );
    }

    #[tokio::test]
    async fn test_load_filtered_by_template() {
        let mut storage = create_in_memory_storage().await;

        let result1 = StrategyResult {
            template_name: "boost_decay_2".to_string(),
            params: "1.0_2.0".to_string(),
            edge: 0.111,
        };
        let result2 = StrategyResult {
            template_name: "boost_decay_2".to_string(),
            params: "3.0_4.0".to_string(),
            edge: 0.222,
        };
        let result3 = StrategyResult {
            template_name: "other_template".to_string(),
            params: "x_y".to_string(),
            edge: 0.333,
        };

        storage.store_result(&result1).await.unwrap();
        storage.store_result(&result2).await.unwrap();
        storage.store_result(&result3).await.unwrap();

        let filtered = storage.load_results(Some("boost_decay_2")).await.unwrap();
        assert_eq!(filtered.len(), 2);
        for res in filtered {
            assert_eq!(res.template_name, "boost_decay_2");
        }

        let empty = storage.load_results(Some("nonexistent")).await.unwrap();
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn test_exists() {
        let mut storage = create_in_memory_storage().await;

        let result = StrategyResult {
            template_name: "boost_decay_2".to_string(),
            params: "0.1_0.2_0.3".to_string(),
            edge: 0.99,
        };

        assert!(
            !storage
                .exists("boost_decay_2", "0.1_0.2_0.3")
                .await
                .unwrap()
        );
        storage.store_result(&result).await.unwrap();
        assert!(
            storage
                .exists("boost_decay_2", "0.1_0.2_0.3")
                .await
                .unwrap()
        );

        // Different template or params should be false.
        assert!(!storage.exists("boost_decay_2", "0.4_0.5").await.unwrap());
        assert!(!storage.exists("other", "0.1_0.2_0.3").await.unwrap());
    }
}
