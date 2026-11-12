//! Cash drawer: open with a float, record cash in/out, close with a count.

use super::Pos;
use crate::error::{invalid, Result};
use crate::models::*;
use crate::money::Money;
use crate::perms::Perm;
use crate::util::now;
use rusqlite::{params, OptionalExtension};

impl Pos {
    fn shift_summary(&self, id: i64) -> Result<ShiftSummary> {
        let (opened_by, opened_at, opening, closed_by, closed_at, counted, notes): (
            String,
            String,
            Money,
            String,
            String,
            Option<Money>,
            String,
        ) = self.conn.query_row(
            "SELECT COALESCE(o.name, ''), s.opened_at, s.opening_cash, COALESCE(c.name, ''), s.closed_at,
                s.counted_cash, s.notes
             FROM shifts s LEFT JOIN users o ON o.id = s.opened_by LEFT JOIN users c ON c.id = s.closed_by
             WHERE s.id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )?;
        let cash_sales: Money = self.conn.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM payments WHERE shift_id = ?1 AND method = 'cash'",
            params![id],
            |r| r.get(0),
        )?;
        let (cash_in, cash_out): (Money, Money) = self.conn.query_row(
            "SELECT COALESCE(SUM(CASE WHEN amount > 0 THEN amount ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN amount < 0 THEN -amount ELSE 0 END), 0)
             FROM cash_moves WHERE shift_id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let end = if closed_at.is_empty() { now() } else { closed_at.clone() };
        let bank_total: Money = self.conn.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM payments WHERE method = 'bank' AND created_at >= ?1 AND created_at <= ?2",
            params![opened_at, end],
            |r| r.get(0),
        )?;
        Ok(ShiftSummary {
            id,
            opened_by,
            opened_at,
            opening_cash: opening,
            cash_sales,
            cash_in,
            cash_out,
            bank_total,
            expected: opening + cash_sales + cash_in - cash_out,
            counted,
            closed_by,
            closed_at,
            notes,
        })
    }

    pub fn current_shift(&self) -> Result<Option<ShiftSummary>> {
        match self.open_shift_id()? {
            Some(id) => Ok(Some(self.shift_summary(id)?)),
            None => Ok(None),
        }
    }

    pub fn open_shift(&self, opening_cash: Money) -> Result<i64> {
        self.require(Perm::CashDrawer)?;
        if opening_cash < 0 {
            return invalid("Opening cash can't be negative.");
        }
        if self.open_shift_id()?.is_some() {
            return invalid("The cash drawer is already open.");
        }
        self.conn.execute(
            "INSERT INTO shifts (opened_by, opened_at, opening_cash) VALUES (?1, ?2, ?3)",
            params![self.uid(), now(), opening_cash],
        )?;
        let id = self.conn.last_insert_rowid();
        self.audit("cash", &format!("Drawer opened with {}", self.money().fmt(opening_cash)));
        Ok(id)
    }

    /// Positive = cash put in, negative = cash taken out (expenses, bank deposit).
    pub fn cash_move(&self, amount: Money, reason: &str) -> Result<()> {
        self.require(Perm::CashDrawer)?;
        if amount == 0 {
            return invalid("Enter an amount.");
        }
        if reason.trim().is_empty() {
            return invalid("Please write a reason.");
        }
        let shift = self.open_shift_id()?.ok_or_else(|| crate::Error::Invalid("Open the cash drawer first.".into()))?;
        self.conn.execute(
            "INSERT INTO cash_moves (shift_id, amount, reason, user_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![shift, amount, reason.trim(), self.uid(), now()],
        )?;
        self.audit("cash", &format!("{} {}", self.money().fmt(amount), reason.trim()));
        Ok(())
    }

    pub fn shift_moves(&self, shift_id: i64) -> Result<Vec<CashMove>> {
        let mut st = self.conn.prepare(
            "SELECT m.id, m.amount, m.reason, COALESCE(u.name, ''), m.created_at
             FROM cash_moves m LEFT JOIN users u ON u.id = m.user_id WHERE m.shift_id = ?1 ORDER BY m.id DESC",
        )?;
        let rows = st
            .query_map(params![shift_id], |r| {
                Ok(CashMove {
                    id: r.get(0)?,
                    amount: r.get(1)?,
                    reason: r.get(2)?,
                    user_name: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn close_shift(&self, counted: Money, notes: &str) -> Result<ShiftSummary> {
        self.require(Perm::CashDrawer)?;
        if counted < 0 {
            return invalid("Counted cash can't be negative.");
        }
        let id = self.open_shift_id()?.ok_or_else(|| crate::Error::Invalid("The cash drawer isn't open.".into()))?;
        let s = self.shift_summary(id)?;
        self.conn.execute(
            "UPDATE shifts SET closed_by = ?1, closed_at = ?2, counted_cash = ?3, expected_cash = ?4, notes = ?5
             WHERE id = ?6",
            params![self.uid(), now(), counted, s.expected, notes.trim(), id],
        )?;
        let out = self.shift_summary(id)?;
        self.audit(
            "cash",
            &format!(
                "Drawer closed. Expected {}, counted {}",
                self.money().fmt(out.expected),
                self.money().fmt(counted)
            ),
        );
        Ok(out)
    }

    pub fn list_shifts(&self, limit: i64) -> Result<Vec<ShiftSummary>> {
        self.require(Perm::CashDrawer)?;
        let mut st = self.conn.prepare("SELECT id FROM shifts ORDER BY id DESC LIMIT ?1")?;
        let ids: Vec<i64> = st.query_map(params![limit], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        ids.into_iter().map(|id| self.shift_summary(id)).collect()
    }

    pub fn last_closed_shift(&self) -> Result<Option<ShiftSummary>> {
        let id: Option<i64> = self
            .conn
            .query_row("SELECT id FROM shifts WHERE closed_at <> '' ORDER BY id DESC LIMIT 1", [], |r| r.get(0))
            .optional()?;
        match id {
            Some(id) => Ok(Some(self.shift_summary(id)?)),
            None => Ok(None),
        }
    }
}
