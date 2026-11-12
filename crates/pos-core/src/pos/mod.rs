//! `Pos` is the single entry point to all business operations.
//! Every method checks the logged-in user's permissions before touching data.

mod cash;
mod catalog;
mod customers;
mod demo;
mod reports;
mod sales;
mod settings;
mod stock;
mod users;
mod vehicles;
mod workshop;

pub use sales::compute_totals;

use crate::db;
use crate::error::{Error, Result};
use crate::models::{Settings, User};
use crate::money::MoneyFmt;
use crate::perms::{self, Perm};
use crate::util::now;
use rusqlite::{params, Connection, OptionalExtension};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

pub struct Pos {
    pub(crate) conn: Connection,
    user: RefCell<Option<User>>,
    settings: RefCell<Settings>,
    path: Option<PathBuf>,
}

impl Pos {
    pub fn open(path: &Path) -> Result<Pos> {
        let conn = db::open(path)?;
        let mut pos = Self::from_conn(conn)?;
        pos.path = Some(path.to_path_buf());
        Ok(pos)
    }

    pub fn open_in_memory() -> Result<Pos> {
        Self::from_conn(db::open_memory()?)
    }

    fn from_conn(conn: Connection) -> Result<Pos> {
        let settings = settings::load(&conn)?;
        Ok(Pos { conn, user: RefCell::new(None), settings: RefCell::new(settings), path: None })
    }

    pub fn db_path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    // ------------------------------------------------------------ session

    pub fn current_user(&self) -> Option<User> {
        self.user.borrow().clone()
    }

    pub(crate) fn uid(&self) -> Option<i64> {
        self.user.borrow().as_ref().map(|u| u.id)
    }

    pub fn can(&self, perm: Perm) -> bool {
        match self.user.borrow().as_ref() {
            Some(u) => perms::can(u.role, perm),
            None => false,
        }
    }

    pub(crate) fn require(&self, perm: Perm) -> Result<()> {
        if self.user.borrow().is_none() {
            return Err(Error::Denied("Please log in first.".into()));
        }
        if self.can(perm) {
            Ok(())
        } else {
            Err(Error::Denied("Your role doesn't allow this. Ask a manager.".into()))
        }
    }

    pub(crate) fn set_user(&self, u: Option<User>) {
        *self.user.borrow_mut() = u;
    }

    pub fn money(&self) -> MoneyFmt {
        let s = self.settings.borrow();
        MoneyFmt {
            symbol: s.currency_symbol.clone(),
            decimals: s.currency_decimals,
            symbol_after: s.symbol_after,
        }
    }

    // ------------------------------------------------------------ helpers

    pub(crate) fn audit(&self, action: &str, detail: &str) {
        let _ = self.conn.execute(
            "INSERT INTO audit_log (user_id, action, detail, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![self.uid(), action, detail, now()],
        );
    }

    /// Next document number, e.g. `INV-000123`.
    pub(crate) fn next_no(&self, prefix: &str) -> Result<String> {
        let v: i64 = self.conn.query_row(
            "INSERT INTO counters (name, value) VALUES (?1, 1)
             ON CONFLICT(name) DO UPDATE SET value = value + 1
             RETURNING value",
            params![prefix],
            |r| r.get(0),
        )?;
        Ok(format!("{prefix}-{v:06}"))
    }

    pub(crate) fn open_shift_id(&self) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM shifts WHERE closed_at = '' ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn audit_log(&self, limit: i64) -> Result<Vec<crate::models::AuditEntry>> {
        self.require(Perm::Settings)?;
        let mut st = self.conn.prepare(
            "SELECT a.id, COALESCE(u.name, 'System'), a.action, a.detail, a.created_at
             FROM audit_log a LEFT JOIN users u ON u.id = a.user_id
             ORDER BY a.id DESC LIMIT ?1",
        )?;
        let rows = st
            .query_map(params![limit], |r| {
                Ok(crate::models::AuditEntry {
                    id: r.get(0)?,
                    user_name: r.get(1)?,
                    action: r.get(2)?,
                    detail: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}

/// Read an optional id column that may be NULL.
pub(crate) fn opt_id(v: Option<i64>) -> Option<i64> {
    v.filter(|x| *x > 0)
}

#[cfg(test)]
mod tests;
