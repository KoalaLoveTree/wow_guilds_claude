/// Database module with SQLite and migrations support
use sqlx::{SqlitePool, Row, sqlite::SqliteQueryResult};
use crate::error::{BotError, Result};
use crate::types::{GuildUrl, GuildName, RealmName, PlayerName};
use std::path::Path;
use tracing::{info, warn, error};

/// Database connection wrapper
#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

/// Guild data structure for database
#[derive(Debug, Clone)]
pub struct DbGuild {
    pub id: i64,
    pub name: String,
    pub realm: String,
    pub url: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Member data structure for database (matches PlayerData JSON structure)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DbMember {
    pub id: i64,
    pub name: String,
    pub realm: String,
    pub guild_name: Option<String>,
    pub guild_realm: Option<String>,
    pub class: Option<String>,
    pub spec: Option<String>,
    pub rio_score: Option<f64>,  // Legacy field - kept for compatibility
    pub ilvl: Option<i32>,
    // RIO fields matching PlayerData structure
    pub rio_all: f64,
    pub rio_dps: f64,
    pub rio_healer: f64,
    pub rio_tank: f64,
    pub spec_0: f64,
    pub spec_1: f64,
    pub spec_2: f64,
    pub spec_3: f64,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Database {
    /// Create a new database connection
    pub async fn new(database_url: &str) -> Result<Self> {
        // SQLx requires specific format for SQLite - create database file if needed
        let database_path = database_url.replace("sqlite://", "");
        let pool = SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&database_path)
                .create_if_missing(true)
        ).await
        .map_err(|e| BotError::Database(format!("Failed to connect to database: {}", e)))?;

        let db = Self { pool };
        db.run_migrations().await?;
        Ok(db)
    }

    /// Run database migrations
    async fn run_migrations(&self) -> Result<()> {
        info!("Running database migrations...");
        
        // Create migrations table
        sqlx::query(r#"
            CREATE TABLE IF NOT EXISTS _migrations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT UNIQUE NOT NULL,
                executed_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )
        "#)
        .execute(&self.pool)
        .await
        .map_err(|e| BotError::Database(format!("Failed to create migrations table: {}", e)))?;

        // Run each migration
        self.migrate_001_create_guilds_table().await?;
        self.migrate_002_create_members_tables().await?;
        self.migrate_003_populate_guild_data().await?;
        self.migrate_004_add_rio_fields_to_members().await?;
        
        info!("Database migrations completed successfully");
        Ok(())
    }

    /// Migration 001: Create guilds table
    async fn migrate_001_create_guilds_table(&self) -> Result<()> {
        let migration_name = "001_create_guilds_table";
        
        if self.migration_exists(migration_name).await? {
            return Ok(());
        }

        info!("Running migration: {}", migration_name);

        sqlx::query(r#"
            CREATE TABLE guilds (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                realm TEXT NOT NULL,
                url TEXT NOT NULL,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(name, realm)
            )
        "#)
        .execute(&self.pool)
        .await
        .map_err(|e| BotError::Database(format!("Migration {} failed: {}", migration_name, e)))?;

        self.record_migration(migration_name).await?;
        Ok(())
    }

    /// Migration 002: Create members tables (active and temporary)
    async fn migrate_002_create_members_tables(&self) -> Result<()> {
        let migration_name = "002_create_members_tables";
        
        if self.migration_exists(migration_name).await? {
            return Ok(());
        }

        info!("Running migration: {}", migration_name);

        // Active members table
        sqlx::query(r#"
            CREATE TABLE members (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                realm TEXT NOT NULL,
                guild_name TEXT,
                guild_realm TEXT,
                class TEXT,
                spec TEXT,
                rio_score REAL,
                ilvl INTEGER,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(name, realm)
            )
        "#)
        .execute(&self.pool)
        .await
        .map_err(|e| BotError::Database(format!("Migration {} failed: {}", migration_name, e)))?;

        // Temporary members table for parsing
        sqlx::query(r#"
            CREATE TABLE members_tmp (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                realm TEXT NOT NULL,
                guild_name TEXT,
                guild_realm TEXT,
                class TEXT,
                spec TEXT,
                rio_score REAL,
                ilvl INTEGER,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(name, realm)
            )
        "#)
        .execute(&self.pool)
        .await
        .map_err(|e| BotError::Database(format!("Migration {} failed: {}", migration_name, e)))?;

        self.record_migration(migration_name).await?;
        Ok(())
    }

    /// Migration 003: Populate guild data
    async fn migrate_003_populate_guild_data(&self) -> Result<()> {
        let migration_name = "003_populate_guild_data";
        
        if self.migration_exists(migration_name).await? {
            return Ok(());
        }

        info!("Running migration: {}", migration_name);

        // Guild data embedded in migration (originally from uaguildlist.txt)
        let guild_data = vec![
            ("Tarren Mill", "Нехай Щастить"),
            ("Tarren Mill", "Wrong Tactics Folks"),
            ("Tarren Mill", "Tauren Milfs"),
            ("Tarren Mill", "Nomads TM"),
            ("Tarren Mill", "The Toxic Avengers"),
            ("Tarren Mill", "Mayhem Soul"),
            ("Tarren Mill", "Millennial Union"),
            ("Tarren Mill", "Draenei Milfs"),
            ("Tarren Mill", "GBK"),
            ("Tarren Mill", "UA Cyborgs"),
            ("Tarren Mill", "Ryan Gosling"),
            ("Tarren Mill", "Order of the Trident"),
            ("Tarren Mill", "EtherealsUA"),
            ("Tarren Mill", "True Men Love"),
            ("Tarren Mill", "Thorned Horde"),
            ("Tarren Mill", "NBU"),
            ("Tarren Mill", "Potujnovodsk"),
            ("Howling Fjord", "Нехай Щастить"),
            ("Howling Fjord", "Бавовна"),
            ("Howling Fjord", "Фортеця"),
            ("Howling Fjord", "Чёрный заслон"),
            ("Howling Fjord", "Дякую за РТ"),
            ("Howling Fjord", "Багряна Вежа"),
            ("Terokkar", "Ukrainian Alliance"),
            ("Terokkar", "Arey"),
            ("Terokkar", "Knaipa Variativ"),
            ("Terokkar", "Komora"),
            ("Terokkar", "Khorugva"),
            ("Terokkar", "Glory to Heroes"),
            ("Terokkar", "Neutral Chaotic"),
            ("Silvermoon", "Mythologeme"),
            ("Silvermoon", "Alphalogeme"),
            ("Silvermoon", "MRIYA"),
            ("Silvermoon", "MOVA"),
            ("Silvermoon", "BAPTA KOTIB"),
            ("Silvermoon", "Synevyr"),
            ("Silvermoon", "Ukraine"),
            ("Silvermoon", "Bcecbit"),
            ("Silvermoon", "Pray for Ukraine"),
            ("Silvermoon", "SNÁFU"),
            ("Silvermoon", "BBC team"),
            ("Silvermoon", "iSHO"),
            ("Silvermoon", "Dark Green"),
            ("Silvermoon", "Wild Field"),
            ("Kazzak", "Borsch Battalion"),
            ("Kazzak", "Hwg"),
            ("Kazzak", "UKRAINIAN GUILD"),
            ("Ravencrest", "Viysko NightElfiyske"),
            ("Ravencrest", "Ababagalamaga"),
            ("Ravencrest", "Tovarystvo Zolotyy Husak"),
            ("Ravencrest", "Unite for Ukraine"),
            ("Twisting Nether", "Morok"),
            ("Draenor", "FavouriteWorstNightmare"),
            ("Draenor", "Ukrainian Cossacks"),
            ("Draenor", "Precedent UA"),
            ("Gordunni", "Героям слава"),
            ("Gordunni", "Гуляйполе"),
            ("Gordunni", "Квента"),
            ("Gordunni", "Эйситерия"),
            ("Eversong", "Харцизи"),
            ("Eversong", "Мы с Украины"),
            ("Soulflayer", "Поляна Квасова"),
        ];

        let guild_count = guild_data.len();
        
        // Insert all guild data
        for (realm, name) in guild_data {
            let url = format!("realm={}&name={}", realm, name);
            
            sqlx::query(r#"
                INSERT OR IGNORE INTO guilds (name, realm, url)
                VALUES (?, ?, ?)
            "#)
            .bind(name)
            .bind(realm)
            .bind(url)
            .execute(&self.pool)
            .await
            .map_err(|e| BotError::Database(format!("Failed to insert guild data: {}", e)))?;
        }

        info!("Populated {} guilds from migration", guild_count);
        self.record_migration(migration_name).await?;
        Ok(())
    }

    /// Migration 004: Add RIO fields to members tables to match JSON structure
    async fn migrate_004_add_rio_fields_to_members(&self) -> Result<()> {
        let migration_name = "004_add_rio_fields_to_members";
        
        if self.migration_exists(migration_name).await? {
            return Ok(());
        }

        info!("Running migration: {}", migration_name);

        // Add missing RIO fields to members table
        let alter_statements = vec![
            "ALTER TABLE members ADD COLUMN rio_all REAL DEFAULT 0",
            "ALTER TABLE members ADD COLUMN rio_dps REAL DEFAULT 0", 
            "ALTER TABLE members ADD COLUMN rio_healer REAL DEFAULT 0",
            "ALTER TABLE members ADD COLUMN rio_tank REAL DEFAULT 0",
            "ALTER TABLE members ADD COLUMN spec_0 REAL DEFAULT 0",
            "ALTER TABLE members ADD COLUMN spec_1 REAL DEFAULT 0", 
            "ALTER TABLE members ADD COLUMN spec_2 REAL DEFAULT 0",
            "ALTER TABLE members ADD COLUMN spec_3 REAL DEFAULT 0",
        ];

        for statement in alter_statements.iter() {
            sqlx::query(statement)
                .execute(&self.pool)
                .await
                .map_err(|e| BotError::Database(format!("Migration {} failed: {}", migration_name, e)))?;
        }

        // Also add the same fields to members_tmp table
        let alter_tmp_statements = vec![
            "ALTER TABLE members_tmp ADD COLUMN rio_all REAL DEFAULT 0",
            "ALTER TABLE members_tmp ADD COLUMN rio_dps REAL DEFAULT 0", 
            "ALTER TABLE members_tmp ADD COLUMN rio_healer REAL DEFAULT 0",
            "ALTER TABLE members_tmp ADD COLUMN rio_tank REAL DEFAULT 0",
            "ALTER TABLE members_tmp ADD COLUMN spec_0 REAL DEFAULT 0",
            "ALTER TABLE members_tmp ADD COLUMN spec_1 REAL DEFAULT 0", 
            "ALTER TABLE members_tmp ADD COLUMN spec_2 REAL DEFAULT 0",
            "ALTER TABLE members_tmp ADD COLUMN spec_3 REAL DEFAULT 0",
        ];

        for statement in alter_tmp_statements.iter() {
            sqlx::query(statement)
                .execute(&self.pool)
                .await
                .map_err(|e| BotError::Database(format!("Migration {} failed: {}", migration_name, e)))?;
        }

        info!("Added RIO fields to members and members_tmp tables");
        self.record_migration(migration_name).await?;
        Ok(())
    }

    /// Check if migration was already executed
    async fn migration_exists(&self, name: &str) -> Result<bool> {
        let result = sqlx::query("SELECT COUNT(*) as count FROM _migrations WHERE name = ?")
            .bind(name)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| BotError::Database(format!("Failed to check migration: {}", e)))?;

        Ok(result.get::<i64, _>("count") > 0)
    }

    /// Record successful migration
    async fn record_migration(&self, name: &str) -> Result<()> {
        sqlx::query("INSERT INTO _migrations (name) VALUES (?)")
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(|e| BotError::Database(format!("Failed to record migration: {}", e)))?;

        Ok(())
    }

    /// Import guild data from uaguildlist.txt
    pub async fn import_guild_data_from_file(&self, file_path: &str) -> Result<usize> {
        if !Path::new(file_path).exists() {
            warn!("Guild list file not found: {}", file_path);
            return Ok(0);
        }

        let content = std::fs::read_to_string(file_path)
            .map_err(|e| BotError::Io(e))?;

        let mut imported = 0;
        let mut errors = 0;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if let Some(guild_url) = self.parse_guild_url(trimmed) {
                match self.insert_guild(&guild_url).await {
                    Ok(_) => imported += 1,
                    Err(e) => {
                        error!("Failed to insert guild {}: {}", trimmed, e);
                        errors += 1;
                    }
                }
            } else {
                warn!("Failed to parse guild URL: {}", trimmed);
                errors += 1;
            }
        }

        info!("Imported {} guilds from {} (errors: {})", imported, file_path, errors);
        Ok(imported)
    }

    /// Parse guild URL from string format
    fn parse_guild_url(&self, url_str: &str) -> Option<GuildUrl> {
        let mut realm = None;
        let mut guild = None;

        for part in url_str.split('&') {
            if let Some((key, value)) = part.split_once('=') {
                match key {
                    "realm" => realm = Some(RealmName::from(value)),
                    "name" => guild = Some(GuildName::from(value)),
                    _ => {}
                }
            }
        }

        match (realm, guild) {
            (Some(realm), Some(name)) => Some(GuildUrl { realm, name }),
            _ => None,
        }
    }

    /// Insert guild into database
    async fn insert_guild(&self, guild_url: &GuildUrl) -> Result<SqliteQueryResult> {
        let url_str = format!("realm={}&name={}", guild_url.realm, guild_url.name);
        
        sqlx::query(r#"
            INSERT OR IGNORE INTO guilds (name, realm, url)
            VALUES (?, ?, ?)
        "#)
        .bind(guild_url.name.to_string())
        .bind(guild_url.realm.to_string())
        .bind(url_str)
        .execute(&self.pool)
        .await
        .map_err(|e| BotError::Database(format!("Failed to insert guild: {}", e)))
    }

    /// Get all guilds from database
    pub async fn get_all_guilds(&self) -> Result<Vec<GuildUrl>> {
        let rows = sqlx::query("SELECT name, realm FROM guilds ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| BotError::Database(format!("Failed to fetch guilds: {}", e)))?;

        let guilds = rows.into_iter().map(|row| {
            GuildUrl {
                name: GuildName::from(row.get::<String, _>("name")),
                realm: RealmName::from(row.get::<String, _>("realm")),
            }
        }).collect();

        Ok(guilds)
    }

    /// Clear temporary members table
    pub async fn clear_temp_members(&self) -> Result<()> {
        sqlx::query("DELETE FROM members_tmp")
            .execute(&self.pool)
            .await
            .map_err(|e| BotError::Database(format!("Failed to clear temp members: {}", e)))?;

        Ok(())
    }

    /// Insert member into temporary table
    pub async fn insert_temp_member(&self, member: &DbMember) -> Result<()> {
        sqlx::query(r#"
            INSERT OR REPLACE INTO members_tmp 
            (name, realm, guild_name, guild_realm, class, spec, rio_score, ilvl, 
             rio_all, rio_dps, rio_healer, rio_tank, spec_0, spec_1, spec_2, spec_3, updated_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#)
        .bind(&member.name)
        .bind(&member.realm)
        .bind(&member.guild_name)
        .bind(&member.guild_realm)
        .bind(&member.class)
        .bind(&member.spec)
        .bind(member.rio_score)
        .bind(member.ilvl)
        .bind(member.rio_all)
        .bind(member.rio_dps)
        .bind(member.rio_healer)
        .bind(member.rio_tank)
        .bind(member.spec_0)
        .bind(member.spec_1)
        .bind(member.spec_2)
        .bind(member.spec_3)
        .bind(member.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| BotError::Database(format!("Failed to insert temp member: {}", e)))?;

        Ok(())
    }

    /// Swap temporary table with active members table
    pub async fn swap_members_tables(&self) -> Result<()> {
        info!("Swapping members tables (tmp -> active)");

        // Use transaction for atomic swap
        let mut tx = self.pool.begin().await
            .map_err(|e| BotError::Database(format!("Failed to start transaction: {}", e)))?;

        // Drop old members table
        sqlx::query("DROP TABLE IF EXISTS members_old")
            .execute(&mut *tx)
            .await
            .map_err(|e| BotError::Database(format!("Failed to drop old table: {}", e)))?;

        // Rename current members to old
        sqlx::query("ALTER TABLE members RENAME TO members_old")
            .execute(&mut *tx)
            .await
            .map_err(|e| BotError::Database(format!("Failed to rename members table: {}", e)))?;

        // Rename tmp to members
        sqlx::query("ALTER TABLE members_tmp RENAME TO members")
            .execute(&mut *tx)
            .await
            .map_err(|e| BotError::Database(format!("Failed to rename tmp table: {}", e)))?;

        // Create new tmp table with all RIO fields
        sqlx::query(r#"
            CREATE TABLE members_tmp (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                realm TEXT NOT NULL,
                guild_name TEXT,
                guild_realm TEXT,
                class TEXT,
                spec TEXT,
                rio_score REAL,
                ilvl INTEGER,
                rio_all REAL DEFAULT 0,
                rio_dps REAL DEFAULT 0,
                rio_healer REAL DEFAULT 0,
                rio_tank REAL DEFAULT 0,
                spec_0 REAL DEFAULT 0,
                spec_1 REAL DEFAULT 0,
                spec_2 REAL DEFAULT 0,
                spec_3 REAL DEFAULT 0,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(name, realm)
            )
        "#)
        .execute(&mut *tx)
        .await
        .map_err(|e| BotError::Database(format!("Failed to create new tmp table: {}", e)))?;

        // Commit transaction
        tx.commit().await
            .map_err(|e| BotError::Database(format!("Failed to commit table swap: {}", e)))?;

        info!("Members table swap completed successfully");
        Ok(())
    }

    /// Get members for rank command
    pub async fn get_members_for_ranking(&self, limit: Option<usize>) -> Result<Vec<DbMember>> {
        let query = if let Some(limit) = limit {
            format!(r#"
                SELECT * FROM members 
                WHERE rio_score IS NOT NULL 
                ORDER BY rio_score DESC 
                LIMIT {}
            "#, limit)
        } else {
            r#"
                SELECT * FROM members 
                WHERE rio_score IS NOT NULL 
                ORDER BY rio_score DESC
            "#.to_string()
        };

        let rows = sqlx::query(&query)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| BotError::Database(format!("Failed to fetch members: {}", e)))?;

        let members = rows.into_iter().map(|row| {
            DbMember {
                id: row.get("id"),
                name: row.get("name"),
                realm: row.get("realm"),
                guild_name: row.get("guild_name"),
                guild_realm: row.get("guild_realm"),
                class: row.get("class"),
                spec: row.get("spec"),
                rio_score: row.get("rio_score"),
                ilvl: row.get("ilvl"),
                rio_all: row.get("rio_all"),
                rio_dps: row.get("rio_dps"),
                rio_healer: row.get("rio_healer"),
                rio_tank: row.get("rio_tank"),
                spec_0: row.get("spec_0"),
                spec_1: row.get("spec_1"),
                spec_2: row.get("spec_2"),
                spec_3: row.get("spec_3"),
                updated_at: row.get("updated_at"),
            }
        }).collect();

        Ok(members)
    }

    /// Get database statistics
    pub async fn get_stats(&self) -> Result<(usize, usize)> {
        let guild_count = sqlx::query("SELECT COUNT(*) as count FROM guilds")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| BotError::Database(format!("Failed to get guild count: {}", e)))?
            .get::<i64, _>("count") as usize;

        let member_count = sqlx::query("SELECT COUNT(*) as count FROM members")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| BotError::Database(format!("Failed to get member count: {}", e)))?
            .get::<i64, _>("count") as usize;

        Ok((guild_count, member_count))
    }

    /// Get list of executed migrations
    pub async fn get_migrations(&self) -> Result<Vec<(String, chrono::DateTime<chrono::Utc>)>> {
        let rows = sqlx::query("SELECT name, executed_at FROM _migrations ORDER BY executed_at")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| BotError::Database(format!("Failed to get migrations: {}", e)))?;

        let migrations = rows.into_iter().map(|row| {
            (
                row.get::<String, _>("name"),
                row.get::<chrono::DateTime<chrono::Utc>, _>("executed_at"),
            )
        }).collect();

        Ok(migrations)
    }
}