use super::Pos;
use crate::error::{invalid, Result};
use crate::models::*;
use crate::money::round_div;
use crate::perms::Perm;
use crate::util::now;
use rusqlite::params;

impl Pos {
    /// Change stock and record why. Used by every stock-affecting operation.
    pub(crate) fn move_stock(
        &self,
        product_id: i64,
        change: i64,
        reason: &str,
        reference: &str,
        unit_cost: i64,
        note: &str,
    ) -> Result<()> {
        self.move_stock_at(product_id, change, reason, reference, unit_cost, note, &now())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn move_stock_at(
        &self,
        product_id: i64,
        change: i64,
        reason: &str,
        reference: &str,
        unit_cost: i64,
        note: &str,
        ts: &str,
    ) -> Result<()> {
        if change == 0 {
            return Ok(());
        }
        self.conn.execute(
            "UPDATE products SET stock = stock + ?1, updated_at = ?2 WHERE id = ?3",
            params![change, ts, product_id],
        )?;
        self.conn.execute(
            "INSERT INTO stock_movements (product_id, change, reason, reference, note, unit_cost, user_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![product_id, change, reason, reference, note, unit_cost, self.uid(), ts],
        )?;
        Ok(())
    }

    /// Manual correction, e.g. damaged or lost items (negative) or found items (positive).
    pub fn adjust_stock(&self, product_id: i64, change: i64, reason: &str, note: &str) -> Result<()> {
        self.require(Perm::AdjustStock)?;
        if change == 0 {
            return invalid("Enter how many to add or remove.");
        }
        let p = self.get_product(product_id)?;
        if p.stock + change < 0 && !self.settings().allow_negative_stock {
            return invalid(format!("Only {} in stock; can't remove {}.", p.stock, -change));
        }
        let tx = self.conn.unchecked_transaction()?;
        let reason = if reason.trim().is_empty() { "adjustment" } else { reason.trim() };
        self.move_stock(product_id, change, reason, "", p.cost, note.trim())?;
        tx.commit()?;
        self.audit("stock", &format!("{} {:+} ({reason})", p.code, change));
        Ok(())
    }

    /// Stocktake: set the counted quantity; the difference is recorded.
    pub fn stocktake(&self, product_id: i64, counted: i64, note: &str) -> Result<i64> {
        self.require(Perm::AdjustStock)?;
        if counted < 0 {
            return invalid("Counted quantity can't be negative.");
        }
        let p = self.get_product(product_id)?;
        let diff = counted - p.stock;
        if diff != 0 {
            let tx = self.conn.unchecked_transaction()?;
            self.move_stock(product_id, diff, "stocktake", "", p.cost, note.trim())?;
            tx.commit()?;
            self.audit("stock", &format!("Stocktake {}: {} → {counted}", p.code, p.stock));
        }
        Ok(diff)
    }

    /// Receive stock from a supplier. Updates stock and the average cost.
    pub fn receive_stock(&self, input: ReceiveInput) -> Result<String> {
        self.receive_stock_at(input, &now())
    }

    pub(crate) fn receive_stock_at(&self, input: ReceiveInput, ts: &str) -> Result<String> {
        self.require(Perm::AdjustStock)?;
        let items: Vec<&ReceiveLine> = input.items.iter().filter(|l| l.qty != 0).collect();
        if items.is_empty() {
            return invalid("Add at least one item to receive.");
        }
        if items.iter().any(|l| l.qty < 0 || l.unit_cost < 0) {
            return invalid("Quantities and costs must be positive.");
        }
        let tx = self.conn.unchecked_transaction()?;
        let po_no = self.next_no("PO")?;
        let total: i64 = items.iter().map(|l| l.qty * l.unit_cost).sum();
        self.conn.execute(
            "INSERT INTO purchases (po_no, supplier_id, total, notes, user_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![po_no, super::opt_id(input.supplier_id), total, input.notes.trim(), self.uid(), ts],
        )?;
        let pid = self.conn.last_insert_rowid();
        for l in items {
            let p = self.get_product(l.product_id)?;
            let on_hand = p.stock.max(0);
            let new_cost = if on_hand + l.qty > 0 {
                round_div(
                    on_hand as i128 * p.cost as i128 + l.qty as i128 * l.unit_cost as i128,
                    (on_hand + l.qty) as i128,
                )
            } else {
                l.unit_cost
            };
            self.conn.execute("UPDATE products SET cost = ?1 WHERE id = ?2", params![new_cost, p.id])?;
            self.conn.execute(
                "INSERT INTO purchase_items (purchase_id, product_id, qty, unit_cost) VALUES (?1, ?2, ?3, ?4)",
                params![pid, p.id, l.qty, l.unit_cost],
            )?;
            self.move_stock_at(p.id, l.qty, "purchase", &po_no, l.unit_cost, "", ts)?;
        }
        tx.commit()?;
        self.audit("stock", &format!("Received stock {po_no}"));
        Ok(po_no)
    }

    pub fn product_movements(&self, product_id: i64, limit: i64) -> Result<Vec<StockMove>> {
        self.require(Perm::Products)?;
        let mut st = self.conn.prepare(
            "SELECT m.id, m.change, m.reason, m.reference, m.note, m.unit_cost, COALESCE(u.name, ''), m.created_at
             FROM stock_movements m LEFT JOIN users u ON u.id = m.user_id
             WHERE m.product_id = ?1 ORDER BY m.id DESC LIMIT ?2",
        )?;
        let rows = st
            .query_map(params![product_id, limit], |r| {
                Ok(StockMove {
                    id: r.get(0)?,
                    change: r.get(1)?,
                    reason: r.get(2)?,
                    reference: r.get(3)?,
                    note: r.get(4)?,
                    unit_cost: r.get(5)?,
                    user_name: r.get(6)?,
                    created_at: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_purchases(&self, limit: i64) -> Result<Vec<PurchaseSummary>> {
        self.require(Perm::AdjustStock)?;
        let mut st = self.conn.prepare(
            "SELECT pu.id, pu.po_no, COALESCE(s.name, '—'), pu.total,
                (SELECT COALESCE(SUM(qty), 0) FROM purchase_items WHERE purchase_id = pu.id), pu.created_at
             FROM purchases pu LEFT JOIN suppliers s ON s.id = pu.supplier_id
             ORDER BY pu.id DESC LIMIT ?1",
        )?;
        let rows = st
            .query_map(params![limit], |r| {
                Ok(PurchaseSummary {
                    id: r.get(0)?,
                    po_no: r.get(1)?,
                    supplier_name: r.get(2)?,
                    total: r.get(3)?,
                    items: r.get(4)?,
                    created_at: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}
