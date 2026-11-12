use super::sales::SaleOpts;
use super::{opt_id, Pos};
use crate::error::{invalid, Error, Result};
use crate::models::*;
use crate::money::Money;
use crate::perms::Perm;
use crate::util::{add_days, now, today};
use rusqlite::{params, params_from_iter, OptionalExtension, Row};

const JOB_COLS: &str = "j.id, j.job_no, j.vehicle_id, v.model_name, COALESCE(v.chassis_no, ''), v.registration_no,
    j.customer_id, COALESCE(c.name, ''), COALESCE(c.phone, ''), j.odometer, j.battery_note, j.complaint,
    j.diagnosis, j.status, j.technician_id, COALESCE(t.name, ''), j.is_warranty, COALESCE(sc.seq, 0), j.approved,
    j.next_service_date, j.next_service_km, j.sale_id, COALESCE(s.invoice_no, ''), j.created_at, j.updated_at,
    j.delivered_at,
    (SELECT COALESCE(SUM(CASE WHEN free = 1 THEN 0 ELSE qty * unit_price END), 0) FROM job_items WHERE job_id = j.id AND kind = 'part'),
    (SELECT COALESCE(SUM(CASE WHEN free = 1 THEN 0 ELSE qty * unit_price END), 0) FROM job_items WHERE job_id = j.id AND kind = 'service')";

const JOB_FROM: &str = "job_cards j JOIN vehicles v ON v.id = j.vehicle_id
    LEFT JOIN customers c ON c.id = j.customer_id LEFT JOIN users t ON t.id = j.technician_id
    LEFT JOIN service_coupons sc ON sc.id = j.coupon_id LEFT JOIN sales s ON s.id = j.sale_id";

fn job_row(r: &Row) -> rusqlite::Result<JobCard> {
    let model: String = r.get(3)?;
    let chassis: String = r.get(4)?;
    let reg: String = r.get(5)?;
    let ident = if !reg.is_empty() { reg.clone() } else { chassis.clone() };
    Ok(JobCard {
        id: r.get(0)?,
        job_no: r.get(1)?,
        vehicle_id: r.get(2)?,
        vehicle_label: if ident.is_empty() { model.clone() } else { format!("{model} · {ident}") },
        model_name: model,
        chassis_no: chassis,
        registration_no: reg,
        customer_id: r.get(6)?,
        customer_name: r.get(7)?,
        customer_phone: r.get(8)?,
        odometer: r.get(9)?,
        battery_note: r.get(10)?,
        complaint: r.get(11)?,
        diagnosis: r.get(12)?,
        status: r.get(13)?,
        technician_id: r.get(14)?,
        technician_name: r.get(15)?,
        is_warranty: r.get(16)?,
        coupon_seq: r.get(17)?,
        approved: r.get(18)?,
        next_service_date: r.get(19)?,
        next_service_km: r.get(20)?,
        sale_id: r.get(21)?,
        invoice_no: r.get(22)?,
        created_at: r.get(23)?,
        updated_at: r.get(24)?,
        delivered_at: r.get(25)?,
        parts_total: r.get(26)?,
        labor_total: r.get(27)?,
        items: Vec::new(),
    })
}

impl Pos {
    pub fn list_jobs(&self, f: &JobFilter) -> Result<Vec<JobCard>> {
        self.require(Perm::Workshop).or_else(|_| self.require(Perm::Customers))?;
        let mut sql = format!("SELECT {JOB_COLS} FROM {JOB_FROM} WHERE 1=1");
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if f.open_only {
            sql.push_str(" AND j.status NOT IN ('delivered','cancelled')");
        }
        if let Some(s) = f.status {
            args.push(s.as_str().to_string().into());
            sql.push_str(&format!(" AND j.status = ?{}", args.len()));
        }
        let q = f.search.trim();
        if !q.is_empty() {
            args.push(format!("%{q}%").into());
            let n = args.len();
            sql.push_str(&format!(
                " AND (j.job_no LIKE ?{n} OR v.chassis_no LIKE ?{n} OR v.registration_no LIKE ?{n}
                   OR v.model_name LIKE ?{n} OR c.name LIKE ?{n} OR c.phone LIKE ?{n} OR j.complaint LIKE ?{n})"
            ));
        }
        sql.push_str(" ORDER BY j.id DESC");
        sql.push_str(&format!(" LIMIT {}", f.limit.unwrap_or(300)));
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params_from_iter(args), job_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_job(&self, id: i64) -> Result<JobCard> {
        let mut job = self
            .conn
            .query_row(&format!("SELECT {JOB_COLS} FROM {JOB_FROM} WHERE j.id = ?1"), params![id], job_row)
            .map_err(|_| Error::NotFound("job card".into()))?;
        let mut st = self.conn.prepare(
            "SELECT id, job_id, kind, product_id, service_id, code, description, qty, unit_price, unit_cost, free
             FROM job_items WHERE job_id = ?1 ORDER BY id",
        )?;
        job.items = st
            .query_map(params![id], |r| {
                Ok(JobItem {
                    id: r.get(0)?,
                    job_id: r.get(1)?,
                    kind: r.get(2)?,
                    product_id: r.get(3)?,
                    service_id: r.get(4)?,
                    code: r.get(5)?,
                    description: r.get(6)?,
                    qty: r.get(7)?,
                    unit_price: r.get(8)?,
                    unit_cost: r.get(9)?,
                    free: r.get(10)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(job)
    }

    pub fn vehicle_history(&self, vehicle_id: i64) -> Result<Vec<JobCard>> {
        let mut st =
            self.conn.prepare(&format!("SELECT {JOB_COLS} FROM {JOB_FROM} WHERE j.vehicle_id = ?1 ORDER BY j.id DESC"))?;
        let rows = st.query_map(params![vehicle_id], job_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub(crate) fn customer_jobs(&self, customer_id: i64) -> Result<Vec<JobCard>> {
        let mut st = self
            .conn
            .prepare(&format!("SELECT {JOB_COLS} FROM {JOB_FROM} WHERE j.customer_id = ?1 ORDER BY j.id DESC LIMIT 200"))?;
        let rows = st.query_map(params![customer_id], job_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn create_job(&self, input: JobInput) -> Result<i64> {
        self.create_job_at(input, &now())
    }

    pub(crate) fn create_job_at(&self, input: JobInput, ts: &str) -> Result<i64> {
        self.require(Perm::Workshop)?;
        let v = self.get_vehicle(input.vehicle_id)?;
        if input.complaint.trim().is_empty() {
            return invalid("Please write the customer's complaint or the work needed.");
        }
        let open: Option<String> = self
            .conn
            .query_row(
                "SELECT job_no FROM job_cards WHERE vehicle_id = ?1 AND status NOT IN ('delivered','cancelled') LIMIT 1",
                params![v.id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(no) = open {
            return invalid(format!("This bike already has an open job card ({no})."));
        }
        let customer_id = opt_id(input.customer_id).or(v.customer_id);
        let tx = self.conn.unchecked_transaction()?;
        let coupon_id: Option<i64> = if input.use_coupon {
            let c: Option<i64> = self
                .conn
                .query_row(
                    "SELECT id FROM service_coupons WHERE vehicle_id = ?1 AND used = 0 ORDER BY seq LIMIT 1",
                    params![v.id],
                    |r| r.get(0),
                )
                .optional()?;
            if c.is_none() {
                return invalid("This bike has no free service coupons left.");
            }
            c
        } else {
            None
        };
        let no = self.next_no("JOB")?;
        self.conn.execute(
            "INSERT INTO job_cards (job_no, vehicle_id, customer_id, odometer, battery_note, complaint, status,
                technician_id, advisor_id, is_warranty, coupon_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'received', ?7, ?8, ?9, ?10, ?11, ?11)",
            params![
                no,
                v.id,
                customer_id,
                input.odometer.max(0),
                input.battery_note.trim(),
                input.complaint.trim(),
                opt_id(input.technician_id),
                self.uid(),
                input.is_warranty,
                coupon_id,
                ts
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        if let Some(c) = coupon_id {
            self.conn.execute(
                "UPDATE service_coupons SET used = 1, job_id = ?1, used_at = ?2 WHERE id = ?3",
                params![id, &ts[..10], c],
            )?;
        }
        tx.commit()?;
        self.audit("job", &format!("Opened {no} for {}", v.label()));
        Ok(id)
    }

    fn open_job(&self, id: i64) -> Result<JobCard> {
        let j = self.get_job(id)?;
        if !j.status.is_open() {
            return invalid(format!("Job {} is closed.", j.job_no));
        }
        Ok(j)
    }

    pub fn update_job(&self, id: i64, u: JobUpdate) -> Result<()> {
        self.require(Perm::Workshop)?;
        self.open_job(id)?;
        self.conn.execute(
            "UPDATE job_cards SET odometer = ?1, battery_note = ?2, complaint = ?3, diagnosis = ?4, technician_id = ?5,
                is_warranty = ?6, approved = ?7, next_service_date = ?8, next_service_km = ?9, updated_at = ?10
             WHERE id = ?11",
            params![
                u.odometer.max(0),
                u.battery_note.trim(),
                u.complaint.trim(),
                u.diagnosis.trim(),
                opt_id(u.technician_id),
                u.is_warranty,
                u.approved,
                u.next_service_date.trim(),
                u.next_service_km.max(0),
                now(),
                id
            ],
        )?;
        Ok(())
    }

    pub fn set_job_status(&self, id: i64, status: JobStatus) -> Result<()> {
        self.require(Perm::Workshop)?;
        let j = self.open_job(id)?;
        if status == JobStatus::Delivered {
            return invalid("Use 'Deliver & bill' to hand the bike back.");
        }
        let tx = self.conn.unchecked_transaction()?;
        if status == JobStatus::Cancelled {
            // Put any parts back into stock and free the coupon.
            for it in j.items.iter().filter(|i| i.kind == JobItemKind::Part) {
                if let Some(pid) = it.product_id {
                    self.move_stock(pid, it.qty, "job_cancel", &j.job_no, it.unit_cost, "")?;
                }
            }
            self.conn.execute(
                "UPDATE service_coupons SET used = 0, job_id = NULL, used_at = '' WHERE job_id = ?1",
                params![id],
            )?;
        }
        self.conn.execute(
            "UPDATE job_cards SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, now(), id],
        )?;
        tx.commit()?;
        self.audit("job", &format!("{} → {}", j.job_no, status.label()));
        Ok(())
    }

    /// Add a part to the job. It is taken out of stock right away.
    pub fn add_job_part(&self, job_id: i64, product_id: i64, qty: i64, free: bool) -> Result<()> {
        self.require(Perm::Workshop)?;
        if qty <= 0 {
            return invalid("Quantity must be at least 1.");
        }
        let j = self.open_job(job_id)?;
        let p = self.get_product(product_id)?;
        if p.stock < qty && !self.settings().allow_negative_stock {
            return invalid(format!("Only {} of {} in stock.", p.stock.max(0), p.name));
        }
        let free = free || j.is_warranty;
        let tx = self.conn.unchecked_transaction()?;
        self.conn.execute(
            "INSERT INTO job_items (job_id, kind, product_id, code, description, qty, unit_price, unit_cost, free, created_at)
             VALUES (?1, 'part', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![job_id, p.id, p.code, p.name, qty, p.price, p.cost, free, now()],
        )?;
        self.move_stock(p.id, -qty, "job", &j.job_no, p.cost, "")?;
        self.conn.execute("UPDATE job_cards SET updated_at = ?1 WHERE id = ?2", params![now(), job_id])?;
        tx.commit()?;
        Ok(())
    }

    /// Add labour. Pass a service from the list, or `None` with your own description and price.
    pub fn add_job_service(
        &self,
        job_id: i64,
        service_id: Option<i64>,
        description: &str,
        price: Money,
        free: bool,
    ) -> Result<()> {
        self.require(Perm::Workshop)?;
        let j = self.open_job(job_id)?;
        let (code, desc, price) = match opt_id(service_id) {
            Some(sid) => {
                let (code, name, list_price): (String, String, Money) = self.conn.query_row(
                    "SELECT code, name, price FROM labor_services WHERE id = ?1",
                    params![sid],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?;
                (code, if description.trim().is_empty() { name } else { description.trim().to_string() }, if price > 0 { price } else { list_price })
            }
            None => {
                if description.trim().is_empty() {
                    return invalid("Describe the work done.");
                }
                (String::new(), description.trim().to_string(), price)
            }
        };
        if price < 0 {
            return invalid("Price can't be negative.");
        }
        let free = free || j.is_warranty || j.coupon_seq > 0 && service_id.is_some() && self.is_first_service(job_id)?;
        self.conn.execute(
            "INSERT INTO job_items (job_id, kind, service_id, code, description, qty, unit_price, unit_cost, free, created_at)
             VALUES (?1, 'service', ?2, ?3, ?4, 1, ?5, 0, ?6, ?7)",
            params![job_id, opt_id(service_id), code, desc, price, free, now()],
        )?;
        self.conn.execute("UPDATE job_cards SET updated_at = ?1 WHERE id = ?2", params![now(), job_id])?;
        Ok(())
    }

    /// A coupon job's first labour line is free (the coupon covers the service charge).
    fn is_first_service(&self, job_id: i64) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM job_items WHERE job_id = ?1 AND kind = 'service' AND free = 1",
            params![job_id],
            |r| r.get(0),
        )?;
        Ok(n == 0)
    }

    pub fn set_job_item_free(&self, item_id: i64, free: bool) -> Result<()> {
        self.require(Perm::Workshop)?;
        let job_id: i64 =
            self.conn.query_row("SELECT job_id FROM job_items WHERE id = ?1", params![item_id], |r| r.get(0))?;
        self.open_job(job_id)?;
        self.conn.execute("UPDATE job_items SET free = ?1 WHERE id = ?2", params![free, item_id])?;
        Ok(())
    }

    pub fn remove_job_item(&self, item_id: i64) -> Result<()> {
        self.require(Perm::Workshop)?;
        let (job_id, kind, product_id, qty, unit_cost): (i64, JobItemKind, Option<i64>, i64, Money) =
            self.conn.query_row(
                "SELECT job_id, kind, product_id, qty, unit_cost FROM job_items WHERE id = ?1",
                params![item_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?;
        let j = self.open_job(job_id)?;
        let tx = self.conn.unchecked_transaction()?;
        if let (JobItemKind::Part, Some(pid)) = (kind, product_id) {
            self.move_stock(pid, qty, "job_return", &j.job_no, unit_cost, "Removed from job")?;
        }
        self.conn.execute("DELETE FROM job_items WHERE id = ?1", params![item_id])?;
        tx.commit()?;
        Ok(())
    }

    /// Hand the bike back: creates the workshop bill and records payment.
    pub fn deliver_job(
        &self,
        job_id: i64,
        discount: Money,
        payments: Vec<PaymentInput>,
        next_service_date: &str,
        next_service_km: i64,
    ) -> Result<SaleReceipt> {
        self.deliver_job_at(job_id, discount, payments, next_service_date, next_service_km, &now())
    }

    pub(crate) fn deliver_job_at(
        &self,
        job_id: i64,
        discount: Money,
        payments: Vec<PaymentInput>,
        next_service_date: &str,
        next_service_km: i64,
        ts: &str,
    ) -> Result<SaleReceipt> {
        self.require(Perm::Workshop)?;
        let j = self.open_job(job_id)?;
        if discount > 0 && !self.can(Perm::Discounts) {
            return Err(Error::Denied("Your role can't give discounts. Ask a manager.".into()));
        }
        let mut lines = Vec::new();
        let mut costs = Vec::new();
        for it in &j.items {
            lines.push(CartLine {
                item_type: match it.kind {
                    JobItemKind::Part => ItemType::Product,
                    JobItemKind::Service => ItemType::Service,
                },
                ref_id: match it.kind {
                    JobItemKind::Part => it.product_id,
                    JobItemKind::Service => it.service_id,
                },
                code: it.code.clone(),
                description: it.description.clone(),
                qty: it.qty,
                unit_price: it.unit_price,
                line_discount: 0,
                serial_no: String::new(),
                free: it.free,
            });
            costs.push(it.unit_cost);
        }
        if lines.is_empty() {
            // Inspection with nothing charged still produces a zero bill for the record.
            lines.push(CartLine {
                item_type: ItemType::Custom,
                ref_id: None,
                code: String::new(),
                description: "Inspection".into(),
                qty: 1,
                unit_price: 0,
                line_discount: 0,
                serial_no: String::new(),
                free: true,
            });
            costs.push(0);
        }
        let input = CheckoutInput {
            kind: SaleKind::Workshop,
            customer_id: j.customer_id,
            salesperson_id: None,
            items: lines,
            discount,
            payments,
            notes: format!("Job {}", j.job_no),
            booking_id: None,
        };
        let next_date = if next_service_date.trim().is_empty() {
            let days = self.settings().next_service_days;
            if days > 0 { add_days(&ts[..10], days) } else { String::new() }
        } else {
            next_service_date.trim().to_string()
        };
        let tx = self.conn.unchecked_transaction()?;
        let receipt = self.checkout_inner(
            &input,
            SaleOpts { ts: Some(ts.to_string()), preset_costs: Some(costs), job_id: Some(job_id) },
        )?;
        self.conn.execute(
            "UPDATE job_cards SET status = 'delivered', sale_id = ?1, delivered_at = ?2, updated_at = ?2,
                next_service_date = ?3, next_service_km = ?4 WHERE id = ?5",
            params![receipt.sale_id, ts, next_date, next_service_km.max(0), job_id],
        )?;
        tx.commit()?;
        self.audit("job", &format!("Delivered {} ({})", j.job_no, receipt.invoice_no));
        Ok(receipt)
    }

    /// Bikes due (or overdue) for their next service within `days`.
    pub fn service_due(&self, days: i64) -> Result<Vec<ServiceDue>> {
        let limit = add_days(&today(), days);
        let mut st = self.conn.prepare(
            "SELECT j.vehicle_id, v.model_name, COALESCE(v.chassis_no, ''), v.registration_no,
                COALESCE(c.name, ''), COALESCE(c.phone, ''), j.next_service_date, j.next_service_km, j.job_no
             FROM job_cards j
             JOIN (SELECT vehicle_id, MAX(id) AS mid FROM job_cards WHERE status = 'delivered' GROUP BY vehicle_id) last
                ON last.mid = j.id
             JOIN vehicles v ON v.id = j.vehicle_id
             LEFT JOIN customers c ON c.id = COALESCE(v.customer_id, j.customer_id)
             WHERE j.next_service_date <> '' AND j.next_service_date <= ?1
               AND NOT EXISTS (SELECT 1 FROM job_cards o WHERE o.vehicle_id = j.vehicle_id AND o.status NOT IN ('delivered','cancelled'))
             ORDER BY j.next_service_date LIMIT 100",
        )?;
        let t = today();
        let rows = st
            .query_map(params![limit], |r| {
                let model: String = r.get(1)?;
                let chassis: String = r.get(2)?;
                let reg: String = r.get(3)?;
                let ident = if reg.is_empty() { chassis } else { reg };
                let due: String = r.get(6)?;
                Ok(ServiceDue {
                    vehicle_id: r.get(0)?,
                    vehicle_label: format!("{model} · {ident}"),
                    customer_name: r.get(4)?,
                    customer_phone: r.get(5)?,
                    overdue: due.as_str() < t.as_str(),
                    due_date: due,
                    due_km: r.get(7)?,
                    last_job_no: r.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
}
