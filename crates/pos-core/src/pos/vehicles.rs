use super::{opt_id, Pos};
use crate::error::{invalid, Error, Result};
use crate::models::*;
use crate::perms::Perm;
use crate::util::{add_months, now, today};
use rusqlite::{params, params_from_iter, Row};

pub(crate) const VEHICLE_COLS: &str = "v.id, v.model_id, v.model_name, COALESCE(v.chassis_no, ''), v.motor_no,
    v.battery_serial, v.color, v.year, v.cost, v.price, v.status, v.customer_id, COALESCE(c.name, ''),
    COALESCE(c.phone, ''), v.registration_no, v.sold_at, v.sale_id, COALESCE(s.invoice_no, ''),
    v.vehicle_warranty_until, v.battery_warranty_until, v.motor_warranty_until, v.warranty_km, v.notes,
    v.received_at,
    (SELECT COUNT(*) FROM service_coupons sc WHERE sc.vehicle_id = v.id AND sc.used = 0)";

pub(crate) const VEHICLE_FROM: &str = "vehicles v LEFT JOIN customers c ON c.id = v.customer_id
    LEFT JOIN sales s ON s.id = v.sale_id";

pub(crate) fn vehicle_row(r: &Row) -> rusqlite::Result<Vehicle> {
    Ok(Vehicle {
        id: r.get(0)?,
        model_id: r.get(1)?,
        model_name: r.get(2)?,
        chassis_no: r.get(3)?,
        motor_no: r.get(4)?,
        battery_serial: r.get(5)?,
        color: r.get(6)?,
        year: r.get(7)?,
        cost: r.get(8)?,
        price: r.get(9)?,
        status: r.get(10)?,
        customer_id: r.get(11)?,
        customer_name: r.get(12)?,
        customer_phone: r.get(13)?,
        registration_no: r.get(14)?,
        sold_at: r.get(15)?,
        sale_id: r.get(16)?,
        invoice_no: r.get(17)?,
        vehicle_warranty_until: r.get(18)?,
        battery_warranty_until: r.get(19)?,
        motor_warranty_until: r.get(20)?,
        warranty_km: r.get(21)?,
        notes: r.get(22)?,
        received_at: r.get(23)?,
        coupons_left: r.get(24)?,
    })
}

const BOOKING_COLS: &str = "b.id, b.booking_no, b.customer_id, c.name, c.phone, b.model_id,
    COALESCE(m.name, v.model_name, ''), b.vehicle_id, COALESCE(v.chassis_no, ''), b.color, b.agreed_price,
    (SELECT COALESCE(SUM(amount), 0) FROM payments WHERE booking_id = b.id),
    b.status, b.expected_date, b.notes, b.created_at";

const BOOKING_FROM: &str = "bookings b JOIN customers c ON c.id = b.customer_id
    LEFT JOIN vehicle_models m ON m.id = b.model_id LEFT JOIN vehicles v ON v.id = b.vehicle_id";

fn booking_row(r: &Row) -> rusqlite::Result<Booking> {
    Ok(Booking {
        id: r.get(0)?,
        booking_no: r.get(1)?,
        customer_id: r.get(2)?,
        customer_name: r.get(3)?,
        customer_phone: r.get(4)?,
        model_id: r.get(5)?,
        model_name: r.get(6)?,
        vehicle_id: r.get(7)?,
        chassis_no: r.get(8)?,
        color: r.get(9)?,
        agreed_price: r.get(10)?,
        advance: r.get(11)?,
        status: r.get(12)?,
        expected_date: r.get(13)?,
        notes: r.get(14)?,
        created_at: r.get(15)?,
    })
}

fn clean(s: &str) -> String {
    s.trim().to_uppercase()
}

impl Pos {
    pub fn list_vehicles(&self, f: &VehicleFilter) -> Result<Vec<Vehicle>> {
        self.require(Perm::Vehicles).or_else(|_| self.require(Perm::Workshop))?;
        let mut sql = format!("SELECT {VEHICLE_COLS} FROM {VEHICLE_FROM} WHERE 1=1");
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(s) = f.status {
            args.push(s.as_str().to_string().into());
            sql.push_str(&format!(" AND v.status = ?{}", args.len()));
        } else if !f.include_external {
            sql.push_str(" AND v.status <> 'external'");
        }
        if let Some(m) = f.model_id {
            args.push(m.into());
            sql.push_str(&format!(" AND v.model_id = ?{}", args.len()));
        }
        let q = f.search.trim();
        if !q.is_empty() {
            args.push(format!("%{q}%").into());
            let n = args.len();
            sql.push_str(&format!(
                " AND (v.chassis_no LIKE ?{n} OR v.motor_no LIKE ?{n} OR v.battery_serial LIKE ?{n}
                   OR v.model_name LIKE ?{n} OR v.registration_no LIKE ?{n} OR v.color LIKE ?{n}
                   OR c.name LIKE ?{n} OR c.phone LIKE ?{n})"
            ));
        }
        sql.push_str(
            " ORDER BY CASE v.status WHEN 'in_stock' THEN 0 WHEN 'booked' THEN 1 WHEN 'demo' THEN 2
               WHEN 'in_transit' THEN 3 WHEN 'sold' THEN 4 ELSE 5 END, v.id DESC LIMIT 1000",
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params_from_iter(args), vehicle_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_vehicle(&self, id: i64) -> Result<Vehicle> {
        self.conn
            .query_row(&format!("SELECT {VEHICLE_COLS} FROM {VEHICLE_FROM} WHERE v.id = ?1"), params![id], vehicle_row)
            .map_err(|_| Error::NotFound("vehicle".into()))
    }

    /// Find bikes for the workshop (any status) by chassis, registration, customer name or phone.
    pub fn find_vehicles(&self, q: &str) -> Result<Vec<Vehicle>> {
        self.require(Perm::Workshop).or_else(|_| self.require(Perm::Vehicles))?;
        let q = q.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let mut st = self.conn.prepare(&format!(
            "SELECT {VEHICLE_COLS} FROM {VEHICLE_FROM}
             WHERE v.status IN ('sold','external','demo') AND (v.chassis_no LIKE ?1 OR v.registration_no LIKE ?1
                OR v.motor_no LIKE ?1 OR c.name LIKE ?1 OR c.phone LIKE ?1 OR v.model_name LIKE ?1)
             ORDER BY v.id DESC LIMIT 20"
        ))?;
        let rows = st.query_map(params![format!("%{q}%")], vehicle_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Add a bike to showroom stock, or edit an existing one.
    pub fn save_vehicle(&self, v: VehicleInput) -> Result<i64> {
        self.require(Perm::EditProducts)?;
        let chassis = clean(&v.chassis_no);
        if chassis.is_empty() {
            return invalid("Please enter the chassis number.");
        }
        let model_name = match opt_id(v.model_id) {
            Some(mid) => self.get_vehicle_model(mid)?.name,
            None => v.model_name.trim().to_string(),
        };
        if model_name.is_empty() {
            return invalid("Please choose the model.");
        }
        if v.cost < 0 || v.price < 0 {
            return invalid("Prices can't be negative.");
        }
        let dup: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM vehicles WHERE chassis_no = ?1 COLLATE NOCASE AND id <> ?2",
            params![chassis, v.id.unwrap_or(0)],
            |r| r.get(0),
        )?;
        if dup > 0 {
            return invalid(format!("Chassis number {chassis} is already in the system."));
        }
        let ts = now();
        match opt_id(v.id) {
            None => {
                let status = match v.status {
                    VehicleStatus::InTransit | VehicleStatus::Demo => v.status,
                    _ => VehicleStatus::InStock,
                };
                self.conn.execute(
                    "INSERT INTO vehicles (model_id, model_name, chassis_no, motor_no, battery_serial, color, year, cost,
                        price, status, registration_no, notes, received_at, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
                    params![
                        opt_id(v.model_id),
                        model_name,
                        chassis,
                        clean(&v.motor_no),
                        clean(&v.battery_serial),
                        v.color.trim(),
                        v.year,
                        v.cost,
                        v.price,
                        status,
                        clean(&v.registration_no),
                        v.notes.trim(),
                        ts
                    ],
                )?;
                let id = self.conn.last_insert_rowid();
                self.audit("vehicle", &format!("Added {model_name} {chassis}"));
                Ok(id)
            }
            Some(id) => {
                let old = self.get_vehicle(id)?;
                if old.cost != v.cost && !self.can(Perm::ViewCost) {
                    return invalid("Your role can't change cost prices.");
                }
                // Only stock statuses can be switched by hand; sold/booked change through sales and bookings.
                let status = match (old.status, v.status) {
                    (
                        VehicleStatus::InStock | VehicleStatus::Demo | VehicleStatus::InTransit,
                        VehicleStatus::InStock | VehicleStatus::Demo | VehicleStatus::InTransit,
                    ) => v.status,
                    _ => old.status,
                };
                self.conn.execute(
                    "UPDATE vehicles SET model_id = ?1, model_name = ?2, chassis_no = ?3, motor_no = ?4,
                        battery_serial = ?5, color = ?6, year = ?7, cost = ?8, price = ?9, status = ?10,
                        registration_no = ?11, notes = ?12 WHERE id = ?13",
                    params![
                        opt_id(v.model_id),
                        model_name,
                        chassis,
                        clean(&v.motor_no),
                        clean(&v.battery_serial),
                        v.color.trim(),
                        v.year,
                        v.cost,
                        v.price,
                        status,
                        clean(&v.registration_no),
                        v.notes.trim(),
                        id
                    ],
                )?;
                Ok(id)
            }
        }
    }

    /// Register a customer's own bike (bought elsewhere) for workshop service.
    pub fn add_customer_bike(
        &self,
        customer_id: i64,
        model_name: &str,
        chassis_no: &str,
        registration_no: &str,
        color: &str,
    ) -> Result<i64> {
        self.require(Perm::Workshop)?;
        if model_name.trim().is_empty() {
            return invalid("Please enter the bike model.");
        }
        let chassis = clean(chassis_no);
        let reg = clean(registration_no);
        if chassis.is_empty() && reg.is_empty() {
            return invalid("Enter the chassis number or the registration number.");
        }
        let chassis_val: Option<String> = if chassis.is_empty() { None } else { Some(chassis) };
        self.conn.execute(
            "INSERT INTO vehicles (model_name, chassis_no, color, status, customer_id, registration_no, created_at)
             VALUES (?1, ?2, ?3, 'external', ?4, ?5, ?6)",
            params![model_name.trim(), chassis_val, color.trim(), customer_id, reg, now()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn set_registration(&self, vehicle_id: i64, registration_no: &str) -> Result<()> {
        self.require(Perm::Vehicles)?;
        self.conn.execute(
            "UPDATE vehicles SET registration_no = ?1 WHERE id = ?2",
            params![clean(registration_no), vehicle_id],
        )?;
        Ok(())
    }

    pub(crate) fn mark_vehicle_sold(&self, v: &Vehicle, customer_id: i64, sale_id: i64, ts: &str) -> Result<()> {
        let date = &ts[..10];
        let m = v.model_id.and_then(|id| self.get_vehicle_model(id).ok());
        let (vw, bw, mw, km, free) = m
            .map(|m| {
                (
                    m.vehicle_warranty_months,
                    m.battery_warranty_months,
                    m.motor_warranty_months,
                    m.warranty_km,
                    m.free_services,
                )
            })
            .unwrap_or((0, 0, 0, 0, 0));
        self.conn.execute(
            "UPDATE vehicles SET status = 'sold', customer_id = ?1, sale_id = ?2, sold_at = ?3,
                vehicle_warranty_until = ?4, battery_warranty_until = ?5, motor_warranty_until = ?6, warranty_km = ?7
             WHERE id = ?8",
            params![
                customer_id,
                sale_id,
                date,
                add_months(date, vw),
                add_months(date, bw),
                add_months(date, mw),
                km,
                v.id
            ],
        )?;
        for seq in 1..=free {
            self.conn.execute(
                "INSERT INTO service_coupons (vehicle_id, seq) VALUES (?1, ?2)",
                params![v.id, seq],
            )?;
        }
        Ok(())
    }

    pub fn vehicle_coupons(&self, vehicle_id: i64) -> Result<Vec<Coupon>> {
        let mut st = self.conn.prepare(
            "SELECT sc.id, sc.vehicle_id, sc.seq, sc.used, COALESCE(j.job_no, ''), sc.used_at
             FROM service_coupons sc LEFT JOIN job_cards j ON j.id = sc.job_id
             WHERE sc.vehicle_id = ?1 ORDER BY sc.seq",
        )?;
        let rows = st
            .query_map(params![vehicle_id], |r| {
                Ok(Coupon {
                    id: r.get(0)?,
                    vehicle_id: r.get(1)?,
                    seq: r.get(2)?,
                    used: r.get(3)?,
                    job_no: r.get(4)?,
                    used_at: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ------------------------------------------------------------ bookings

    pub fn list_bookings(&self, status: Option<BookingStatus>) -> Result<Vec<Booking>> {
        self.require(Perm::Vehicles)?;
        let mut st = self.conn.prepare(&format!(
            "SELECT {BOOKING_COLS} FROM {BOOKING_FROM} WHERE (?1 IS NULL OR b.status = ?1) ORDER BY b.id DESC LIMIT 500"
        ))?;
        let rows = st
            .query_map(params![status.map(|s| s.as_str())], booking_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_booking(&self, id: i64) -> Result<Booking> {
        Ok(self.conn.query_row(
            &format!("SELECT {BOOKING_COLS} FROM {BOOKING_FROM} WHERE b.id = ?1"),
            params![id],
            booking_row,
        )?)
    }

    pub(crate) fn customer_bookings(&self, customer_id: i64) -> Result<Vec<Booking>> {
        let mut st = self.conn.prepare(&format!(
            "SELECT {BOOKING_COLS} FROM {BOOKING_FROM} WHERE b.customer_id = ?1 ORDER BY b.id DESC"
        ))?;
        let rows = st.query_map(params![customer_id], booking_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Book a bike with an advance payment. If a specific bike is chosen it is reserved.
    pub fn create_booking(&self, b: BookingInput) -> Result<String> {
        self.create_booking_at(b, &now())
    }

    pub(crate) fn create_booking_at(&self, b: BookingInput, ts: &str) -> Result<String> {
        self.require(Perm::SellVehicles)?;
        if b.customer_id <= 0 {
            return invalid("Please choose the customer.");
        }
        if opt_id(b.model_id).is_none() && opt_id(b.vehicle_id).is_none() {
            return invalid("Choose the model or a specific bike.");
        }
        if b.advance.iter().any(|p| p.amount < 0) {
            return invalid("Advance can't be negative.");
        }
        let mut model_id = opt_id(b.model_id);
        let tx = self.conn.unchecked_transaction()?;
        if let Some(vid) = opt_id(b.vehicle_id) {
            let v = self.get_vehicle(vid)?;
            if !matches!(v.status, VehicleStatus::InStock | VehicleStatus::Demo | VehicleStatus::InTransit) {
                return invalid(format!("Bike {} is not available to book.", v.chassis_no));
            }
            model_id = model_id.or(v.model_id);
            if v.status != VehicleStatus::InTransit {
                self.conn.execute("UPDATE vehicles SET status = 'booked' WHERE id = ?1", params![vid])?;
            }
        }
        let no = self.next_no("BK")?;
        self.conn.execute(
            "INSERT INTO bookings (booking_no, customer_id, model_id, vehicle_id, color, agreed_price, status,
                expected_date, notes, user_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'open', ?7, ?8, ?9, ?10)",
            params![
                no,
                b.customer_id,
                model_id,
                opt_id(b.vehicle_id),
                b.color.trim(),
                b.agreed_price,
                b.expected_date.trim(),
                b.notes.trim(),
                self.uid(),
                ts
            ],
        )?;
        let bid = self.conn.last_insert_rowid();
        for p in b.advance.iter().filter(|p| p.amount > 0) {
            self.insert_payment(None, Some(bid), Some(b.customer_id), p, ts)?;
        }
        tx.commit()?;
        self.audit("booking", &format!("Created {no}"));
        Ok(no)
    }

    pub fn add_booking_payment(&self, booking_id: i64, p: PaymentInput) -> Result<()> {
        self.require(Perm::SellVehicles)?;
        if p.amount <= 0 {
            return invalid("Enter the amount received.");
        }
        let b = self.get_booking(booking_id)?;
        if b.status != BookingStatus::Open {
            return invalid("This booking is closed.");
        }
        self.insert_payment(None, Some(booking_id), Some(b.customer_id), &p, &now())?;
        self.audit("booking", &format!("Payment on {}", b.booking_no));
        Ok(())
    }

    /// Assign (or change) the exact bike for an open booking.
    pub fn assign_booking_vehicle(&self, booking_id: i64, vehicle_id: i64) -> Result<()> {
        self.require(Perm::SellVehicles)?;
        let b = self.get_booking(booking_id)?;
        if b.status != BookingStatus::Open {
            return invalid("This booking is closed.");
        }
        let v = self.get_vehicle(vehicle_id)?;
        if !matches!(v.status, VehicleStatus::InStock | VehicleStatus::Demo) {
            return invalid(format!("Bike {} is not available.", v.chassis_no));
        }
        let tx = self.conn.unchecked_transaction()?;
        if let Some(old) = b.vehicle_id {
            self.conn.execute(
                "UPDATE vehicles SET status = 'in_stock' WHERE id = ?1 AND status = 'booked'",
                params![old],
            )?;
        }
        self.conn.execute("UPDATE vehicles SET status = 'booked' WHERE id = ?1", params![vehicle_id])?;
        self.conn.execute(
            "UPDATE bookings SET vehicle_id = ?1, model_id = COALESCE(model_id, ?2) WHERE id = ?3",
            params![vehicle_id, v.model_id, booking_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Cancel a booking. The bike goes back to stock; the advance can be refunded.
    pub fn cancel_booking(&self, booking_id: i64, refund: Option<PayMethod>) -> Result<()> {
        self.require(Perm::SellVehicles)?;
        let b = self.get_booking(booking_id)?;
        if b.status != BookingStatus::Open {
            return invalid("This booking is already closed.");
        }
        let tx = self.conn.unchecked_transaction()?;
        if let Some(vid) = b.vehicle_id {
            self.conn.execute(
                "UPDATE vehicles SET status = 'in_stock' WHERE id = ?1 AND status = 'booked'",
                params![vid],
            )?;
        }
        if let (Some(method), true) = (refund, b.advance > 0) {
            let p = PaymentInput {
                method,
                amount: -b.advance,
                bank_name: String::new(),
                reference: b.booking_no.clone(),
                note: "Booking refund".into(),
            };
            self.insert_payment(None, Some(booking_id), Some(b.customer_id), &p, &now())?;
        }
        self.conn.execute("UPDATE bookings SET status = 'cancelled' WHERE id = ?1", params![booking_id])?;
        tx.commit()?;
        self.audit("booking", &format!("Cancelled {}", b.booking_no));
        Ok(())
    }

    /// Bikes whose warranty is still running.
    pub fn warranty_active(&self, until: &str) -> bool {
        !until.is_empty() && until >= today().as_str()
    }
}
