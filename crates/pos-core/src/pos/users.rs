use super::Pos;
use crate::error::{invalid, Error, Result};
use crate::models::{Role, User, UserInput};
use crate::perms::Perm;
use crate::util::now;
use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rusqlite::{params, Row};

const USER_COLS: &str = "id, name, role, phone, active, pin_hash IS NOT NULL AND pin_hash <> '', created_at";

fn user_row(r: &Row) -> rusqlite::Result<User> {
    Ok(User {
        id: r.get(0)?,
        name: r.get(1)?,
        role: r.get(2)?,
        phone: r.get(3)?,
        active: r.get(4)?,
        has_pin: r.get(5)?,
        created_at: r.get(6)?,
    })
}

fn hash_pin(pin: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(pin.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| Error::Invalid(format!("Could not secure PIN: {e}")))
}

fn check_pin(pin: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default().verify_password(pin.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

fn validate_pin(pin: &str) -> Result<()> {
    if pin.len() < 4 || pin.len() > 8 || !pin.chars().all(|c| c.is_ascii_digit()) {
        return invalid("PIN must be 4 to 8 digits.");
    }
    Ok(())
}

impl Pos {
    /// Users who can log in (active with a PIN). No permission needed: used by the login screen.
    pub fn login_users(&self) -> Result<Vec<User>> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {USER_COLS} FROM users WHERE active = 1 AND pin_hash IS NOT NULL AND pin_hash <> '' ORDER BY
             CASE role WHEN 'owner' THEN 0 WHEN 'manager' THEN 1 ELSE 2 END, name"
        ))?;
        let rows = st.query_map([], user_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_users(&self, include_inactive: bool) -> Result<Vec<User>> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {USER_COLS} FROM users WHERE (?1 OR active = 1) ORDER BY active DESC, name"
        ))?;
        let rows = st.query_map(params![include_inactive], user_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Staff who can be assigned to workshop jobs.
    pub fn technicians(&self) -> Result<Vec<User>> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {USER_COLS} FROM users WHERE active = 1 AND role IN ('technician','advisor','manager','owner')
             ORDER BY CASE role WHEN 'technician' THEN 0 ELSE 1 END, name"
        ))?;
        let rows = st.query_map([], user_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Staff who can be credited for a sale.
    pub fn salespeople(&self) -> Result<Vec<User>> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {USER_COLS} FROM users WHERE active = 1 AND role <> 'technician' ORDER BY name"
        ))?;
        let rows = st.query_map([], user_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_user(&self, id: i64) -> Result<User> {
        Ok(self
            .conn
            .query_row(&format!("SELECT {USER_COLS} FROM users WHERE id = ?1"), params![id], user_row)?)
    }

    pub fn login(&self, user_id: i64, pin: &str) -> Result<User> {
        let (hash, active): (Option<String>, bool) = self
            .conn
            .query_row("SELECT pin_hash, active FROM users WHERE id = ?1", params![user_id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(|_| Error::Invalid("User not found.".into()))?;
        let ok = active && hash.as_deref().map(|h| check_pin(pin, h)).unwrap_or(false);
        if !ok {
            return invalid("Wrong PIN. Please try again.");
        }
        let user = self.get_user(user_id)?;
        self.set_user(Some(user.clone()));
        self.audit("login", &user.name);
        Ok(user)
    }

    pub fn logout(&self) {
        if let Some(u) = self.current_user() {
            self.audit("logout", &u.name);
        }
        self.set_user(None);
    }

    /// Check a manager/owner PIN (for approving restricted actions).
    pub fn verify_manager_pin(&self, pin: &str) -> Result<User> {
        let mut st = self.conn.prepare(
            "SELECT id, pin_hash FROM users WHERE active = 1 AND role IN ('owner','manager') AND pin_hash IS NOT NULL",
        )?;
        let rows: Vec<(i64, String)> =
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        for (id, h) in rows {
            if check_pin(pin, &h) {
                return self.get_user(id);
            }
        }
        invalid("That PIN doesn't belong to a manager or owner.")
    }

    pub(crate) fn insert_user(&self, u: &UserInput) -> Result<i64> {
        let role = u.role.unwrap_or(Role::Cashier);
        let hash = match u.pin.as_deref() {
            Some(p) if !p.is_empty() => {
                validate_pin(p)?;
                Some(hash_pin(p)?)
            }
            _ => None,
        };
        if role != Role::Technician && hash.is_none() {
            return invalid("Please set a PIN so this person can log in.");
        }
        self.conn.execute(
            "INSERT INTO users (name, role, phone, pin_hash, active, created_at) VALUES (?1, ?2, ?3, ?4, 1, ?5)",
            params![u.name.trim(), role, u.phone.trim(), hash, now()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn save_user(&self, u: UserInput) -> Result<i64> {
        self.require(Perm::Staff)?;
        if u.name.trim().is_empty() {
            return invalid("Please enter a name.");
        }
        let role = u.role.unwrap_or(Role::Cashier);
        match u.id {
            None => {
                let id = self.insert_user(&u)?;
                self.audit("staff", &format!("Added {} ({})", u.name.trim(), role.label()));
                Ok(id)
            }
            Some(id) => {
                let existing = self.get_user(id)?;
                // Never leave the shop without an active owner.
                if existing.role == Role::Owner && (role != Role::Owner || !u.active) {
                    let owners: i64 = self.conn.query_row(
                        "SELECT COUNT(*) FROM users WHERE role = 'owner' AND active = 1 AND id <> ?1",
                        params![id],
                        |r| r.get(0),
                    )?;
                    if owners == 0 {
                        return invalid("There must always be at least one active owner.");
                    }
                }
                self.conn.execute(
                    "UPDATE users SET name = ?1, role = ?2, phone = ?3, active = ?4 WHERE id = ?5",
                    params![u.name.trim(), role, u.phone.trim(), u.active, id],
                )?;
                if let Some(pin) = u.pin.as_deref() {
                    if pin.is_empty() {
                        if role != Role::Technician {
                            return invalid("Only technicians can be saved without a PIN.");
                        }
                        self.conn.execute("UPDATE users SET pin_hash = NULL WHERE id = ?1", params![id])?;
                    } else {
                        validate_pin(pin)?;
                        let h = hash_pin(pin)?;
                        self.conn.execute("UPDATE users SET pin_hash = ?1 WHERE id = ?2", params![h, id])?;
                    }
                }
                self.audit("staff", &format!("Updated {}", u.name.trim()));
                Ok(id)
            }
        }
    }

    /// Let the logged-in user change their own PIN.
    pub fn change_own_pin(&self, old_pin: &str, new_pin: &str) -> Result<()> {
        let me = self.current_user().ok_or_else(|| Error::Denied("Please log in first.".into()))?;
        let hash: Option<String> =
            self.conn.query_row("SELECT pin_hash FROM users WHERE id = ?1", params![me.id], |r| r.get(0))?;
        if !hash.as_deref().map(|h| check_pin(old_pin, h)).unwrap_or(false) {
            return invalid("Current PIN is wrong.");
        }
        validate_pin(new_pin)?;
        let h = hash_pin(new_pin)?;
        self.conn.execute("UPDATE users SET pin_hash = ?1 WHERE id = ?2", params![h, me.id])?;
        self.audit("staff", "Changed own PIN");
        Ok(())
    }
}
