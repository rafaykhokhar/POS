use super::Pos;
use crate::error::{invalid, Result};
use crate::models::{Role, Settings, SetupInput, UserInput};
use crate::perms::Perm;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub(super) fn load(conn: &Connection) -> Result<Settings> {
    let json: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = 'app'", [], |r| r.get(0))
        .optional()?;
    Ok(json.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default())
}

impl Pos {
    pub fn settings(&self) -> Settings {
        self.settings.borrow().clone()
    }

    pub fn is_setup_done(&self) -> bool {
        self.conn
            .query_row("SELECT COUNT(*) FROM users WHERE role = 'owner'", [], |r| r.get::<_, i64>(0))
            .map(|n| n > 0)
            .unwrap_or(false)
    }

    fn write_settings(&self, s: &Settings) -> Result<()> {
        let json = serde_json::to_string(s).expect("settings serialize");
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES ('app', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![json],
        )?;
        *self.settings.borrow_mut() = s.clone();
        Ok(())
    }

    pub fn save_settings(&self, s: Settings) -> Result<()> {
        self.require(Perm::Settings)?;
        validate(&s)?;
        self.write_settings(&s)?;
        self.audit("settings", "Settings updated");
        Ok(())
    }

    /// Theme can be changed by anyone.
    pub fn set_dark_theme(&self, dark: bool) -> Result<()> {
        let mut s = self.settings();
        s.dark_theme = dark;
        self.write_settings(&s)
    }

    /// First-run setup: shop details and the owner account. Logs the owner in.
    pub fn complete_setup(&self, input: SetupInput) -> Result<()> {
        if self.is_setup_done() {
            return invalid("Setup has already been completed.");
        }
        if input.settings.shop_name.trim().is_empty() {
            return invalid("Please enter the shop name.");
        }
        if input.owner_name.trim().is_empty() {
            return invalid("Please enter the owner's name.");
        }
        validate(&input.settings)?;
        let tx = self.conn.unchecked_transaction()?;
        self.write_settings(&input.settings)?;
        let id = self.insert_user(&UserInput {
            id: None,
            name: input.owner_name.trim().into(),
            role: Some(Role::Owner),
            phone: String::new(),
            pin: Some(input.owner_pin.clone()),
            active: true,
        })?;
        tx.commit()?;
        let owner = self.get_user(id)?;
        self.set_user(Some(owner));
        self.audit("setup", "Shop set up");
        if input.demo_data {
            self.load_demo_data()?;
        }
        Ok(())
    }

    /// Copy the whole database to `dest` (safe while the app is running).
    pub fn backup_to(&self, dest: &Path) -> Result<()> {
        if dest.exists() {
            std::fs::remove_file(dest)?;
        }
        self.conn.execute("VACUUM INTO ?1", params![dest.to_string_lossy()])?;
        Ok(())
    }

    /// Daily automatic backup into `dir`, keeping the newest `keep` files.
    pub fn auto_backup(&self, dir: &Path, keep: usize) -> Result<Option<std::path::PathBuf>> {
        std::fs::create_dir_all(dir)?;
        let name = format!("backup-{}.db", crate::util::today());
        let dest = dir.join(&name);
        if dest.exists() {
            return Ok(None);
        }
        self.backup_to(&dest)?;
        let mut files: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("backup-") && n.ends_with(".db"))
                    .unwrap_or(false)
            })
            .collect();
        files.sort();
        while files.len() > keep {
            let old = files.remove(0);
            let _ = std::fs::remove_file(old);
        }
        Ok(Some(dest))
    }
}

fn validate(s: &Settings) -> Result<()> {
    if s.currency_decimals > 2 {
        return invalid("Currency decimals must be 0, 1 or 2.");
    }
    if !(0..=10_000).contains(&s.tax_rate_bp) {
        return invalid("Tax rate must be between 0% and 100%.");
    }
    if s.low_stock_default < 0 {
        return invalid("Low stock level can't be negative.");
    }
    Ok(())
}
