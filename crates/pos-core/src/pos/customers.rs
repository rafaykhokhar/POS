use super::sales::{payment_row, sale_row, PAYMENT_COLS, PAYMENT_FROM, SALE_COLS, SALE_FROM};
use super::vehicles::{vehicle_row, VEHICLE_COLS, VEHICLE_FROM};
use super::{opt_id, Pos};
use crate::error::{invalid, Result};
use crate::models::*;
use crate::money::Money;
use crate::perms::Perm;
use crate::util::now;
use rusqlite::{params, Row};

const CUSTOMER_COLS: &str = "c.id, c.name, c.phone, c.id_no, c.address, c.email, c.notes, c.created_at,
    (SELECT COALESCE(SUM(s.total), 0) FROM sales s WHERE s.customer_id = c.id AND s.status = 'completed')
      - (SELECT COALESCE(SUM(p.amount), 0) FROM payments p JOIN sales s ON s.id = p.sale_id
         WHERE s.customer_id = c.id AND s.status = 'completed') AS balance,
    (SELECT COUNT(*) FROM vehicles v WHERE v.customer_id = c.id) AS vehicle_count,
    (SELECT COALESCE(SUM(s.total), 0) FROM sales s WHERE s.customer_id = c.id AND s.status = 'completed') AS spent";

fn customer_row(r: &Row) -> rusqlite::Result<Customer> {
    Ok(Customer {
        id: r.get(0)?,
        name: r.get(1)?,
        phone: r.get(2)?,
        id_no: r.get(3)?,
        address: r.get(4)?,
        email: r.get(5)?,
        notes: r.get(6)?,
        created_at: r.get(7)?,
        balance: r.get(8)?,
        vehicles: r.get(9)?,
        total_spent: r.get(10)?,
    })
}

impl Pos {
    pub fn list_customers(&self, search: &str, only_owing: bool, limit: i64) -> Result<Vec<Customer>> {
        self.require(Perm::Customers)
            .or_else(|_| self.require(Perm::Counter))
            .or_else(|_| self.require(Perm::Workshop))?;
        let q = search.trim();
        let filter = if q.is_empty() { "" } else { " WHERE c.name LIKE ?1 OR c.phone LIKE ?1 OR c.id_no LIKE ?1" };
        let mut sql = format!("SELECT * FROM (SELECT {CUSTOMER_COLS} FROM customers c{filter}) t");
        if only_owing {
            sql.push_str(" WHERE t.balance > 0");
        }
        sql.push_str(&format!(" ORDER BY t.name COLLATE NOCASE LIMIT {limit}"));
        let mut st = self.conn.prepare(&sql)?;
        let rows = if q.is_empty() {
            st.query_map([], customer_row)?.collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            st.query_map(params![format!("%{q}%")], customer_row)?.collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(rows)
    }

    pub fn get_customer(&self, id: i64) -> Result<Customer> {
        Ok(self.conn.query_row(
            &format!("SELECT {CUSTOMER_COLS} FROM customers c WHERE c.id = ?1"),
            params![id],
            customer_row,
        )?)
    }

    pub fn save_customer(&self, c: CustomerInput) -> Result<i64> {
        self.require(Perm::Customers)
            .or_else(|_| self.require(Perm::Counter))
            .or_else(|_| self.require(Perm::Workshop))?;
        if c.name.trim().is_empty() {
            return invalid("Please enter the customer's name.");
        }
        match opt_id(c.id) {
            None => {
                self.conn.execute(
                    "INSERT INTO customers (name, phone, id_no, address, email, notes, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        c.name.trim(),
                        c.phone.trim(),
                        c.id_no.trim(),
                        c.address.trim(),
                        c.email.trim(),
                        c.notes.trim(),
                        now()
                    ],
                )?;
                Ok(self.conn.last_insert_rowid())
            }
            Some(id) => {
                self.conn.execute(
                    "UPDATE customers SET name = ?1, phone = ?2, id_no = ?3, address = ?4, email = ?5, notes = ?6
                     WHERE id = ?7",
                    params![
                        c.name.trim(),
                        c.phone.trim(),
                        c.id_no.trim(),
                        c.address.trim(),
                        c.email.trim(),
                        c.notes.trim(),
                        id
                    ],
                )?;
                Ok(id)
            }
        }
    }

    pub fn customer_detail(&self, id: i64) -> Result<CustomerDetail> {
        self.require(Perm::Customers).or_else(|_| self.require(Perm::Workshop))?;
        let customer = self.get_customer(id)?;
        let mut st = self.conn.prepare(&format!(
            "SELECT {VEHICLE_COLS} FROM {VEHICLE_FROM} WHERE v.customer_id = ?1 ORDER BY v.id DESC"
        ))?;
        let vehicles = st.query_map(params![id], vehicle_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut st = self.conn.prepare(&format!(
            "SELECT {SALE_COLS} FROM {SALE_FROM} WHERE s.customer_id = ?1 ORDER BY s.id DESC LIMIT 300"
        ))?;
        let sales = st.query_map(params![id], sale_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut st = self.conn.prepare(&format!(
            "SELECT {PAYMENT_COLS} FROM {PAYMENT_FROM}
             WHERE p.customer_id = ?1 OR s.customer_id = ?1 ORDER BY p.id DESC LIMIT 300"
        ))?;
        let payments = st.query_map(params![id], payment_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        let jobs = self.customer_jobs(id)?;
        let bookings = self.customer_bookings(id)?;
        Ok(CustomerDetail { customer, vehicles, sales, jobs, payments, bookings })
    }

    /// Receive money towards what the customer owes. Paid against the oldest bills first.
    pub fn receive_customer_payment(&self, customer_id: i64, p: PaymentInput) -> Result<()> {
        self.require(Perm::Customers).or_else(|_| self.require(Perm::Counter))?;
        if p.amount <= 0 {
            return invalid("Enter the amount received.");
        }
        let mut st = self.conn.prepare(
            "SELECT s.id, s.total - (SELECT COALESCE(SUM(amount), 0) FROM payments WHERE sale_id = s.id) AS due
             FROM sales s WHERE s.customer_id = ?1 AND s.status = 'completed' AND s.total > 0
             ORDER BY s.created_at, s.id",
        )?;
        let dues: Vec<(i64, Money)> = st
            .query_map(params![customer_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|(_, d)| *d > 0)
            .collect();
        let owed: Money = self.get_customer(customer_id)?.balance;
        if owed <= 0 {
            return invalid("This customer doesn't owe anything.");
        }
        if p.amount > owed {
            return invalid(format!("That's more than the balance owed ({}).", self.money().fmt(owed)));
        }
        let ts = now();
        let tx = self.conn.unchecked_transaction()?;
        let mut left = p.amount;
        for (sid, due) in dues {
            if left <= 0 {
                break;
            }
            let pay = left.min(due);
            let mut part = p.clone();
            part.amount = pay;
            if part.note.is_empty() {
                part.note = "Balance payment".into();
            }
            self.insert_payment(Some(sid), None, Some(customer_id), &part, &ts)?;
            left -= pay;
        }
        tx.commit()?;
        self.audit("payment", &format!("Customer #{customer_id} paid {}", self.money().fmt(p.amount)));
        Ok(())
    }
}
