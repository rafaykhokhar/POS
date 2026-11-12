use super::{opt_id, Pos};
use crate::error::{invalid, Result};
use crate::models::*;
use crate::perms::Perm;
use crate::util::{clean_code, now};
use rusqlite::{params, params_from_iter, OptionalExtension, Row};

pub(crate) const PRODUCT_COLS: &str = "p.id, p.code, COALESCE(p.barcode, ''), p.name, p.category_id, COALESCE(c.name, ''),
    p.kind, p.brand, p.unit, p.cost, p.price, p.stock, p.low_stock, p.fits_models, p.location,
    p.serial_tracked, p.warranty_months, p.active, p.notes";

pub(crate) const PRODUCT_FROM: &str = "products p LEFT JOIN categories c ON c.id = p.category_id";

pub(crate) fn product_row(r: &Row) -> rusqlite::Result<Product> {
    Ok(Product {
        id: r.get(0)?,
        code: r.get(1)?,
        barcode: r.get(2)?,
        name: r.get(3)?,
        category_id: r.get(4)?,
        category_name: r.get(5)?,
        kind: r.get(6)?,
        brand: r.get(7)?,
        unit: r.get(8)?,
        cost: r.get(9)?,
        price: r.get(10)?,
        stock: r.get(11)?,
        low_stock: r.get(12)?,
        fits_models: r.get(13)?,
        location: r.get(14)?,
        serial_tracked: r.get(15)?,
        warranty_months: r.get(16)?,
        active: r.get(17)?,
        notes: r.get(18)?,
    })
}

fn model_row(r: &Row) -> rusqlite::Result<VehicleModel> {
    Ok(VehicleModel {
        id: r.get(0)?,
        name: r.get(1)?,
        brand: r.get(2)?,
        price: r.get(3)?,
        cost: r.get(4)?,
        vehicle_warranty_months: r.get(5)?,
        battery_warranty_months: r.get(6)?,
        motor_warranty_months: r.get(7)?,
        warranty_km: r.get(8)?,
        free_services: r.get(9)?,
        active: r.get(10)?,
        in_stock: r.get(11)?,
    })
}

/// Default code prefix from a category name: "Brake Pads" -> "BRA".
fn auto_prefix(name: &str) -> String {
    let letters: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).take(3).collect();
    if letters.is_empty() {
        "P".into()
    } else {
        letters.to_uppercase()
    }
}

impl Pos {
    // ------------------------------------------------------------ categories

    pub fn list_categories(&self) -> Result<Vec<Category>> {
        let mut st = self.conn.prepare(
            "SELECT c.id, c.name, c.prefix, (SELECT COUNT(*) FROM products p WHERE p.category_id = c.id AND p.active = 1)
             FROM categories c ORDER BY c.name COLLATE NOCASE",
        )?;
        let rows = st
            .query_map([], |r| {
                Ok(Category { id: r.get(0)?, name: r.get(1)?, prefix: r.get(2)?, product_count: r.get(3)? })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn save_category(&self, id: Option<i64>, name: &str, prefix: &str) -> Result<i64> {
        self.require(Perm::EditProducts)?;
        let name = name.trim();
        if name.is_empty() {
            return invalid("Please enter a category name.");
        }
        let prefix = clean_code(prefix);
        match opt_id(id) {
            None => {
                self.conn
                    .execute("INSERT INTO categories (name, prefix) VALUES (?1, ?2)", params![name, prefix])?;
                Ok(self.conn.last_insert_rowid())
            }
            Some(id) => {
                self.conn.execute(
                    "UPDATE categories SET name = ?1, prefix = ?2 WHERE id = ?3",
                    params![name, prefix, id],
                )?;
                Ok(id)
            }
        }
    }

    pub fn delete_category(&self, id: i64) -> Result<()> {
        self.require(Perm::EditProducts)?;
        self.conn.execute("UPDATE products SET category_id = NULL WHERE category_id = ?1", params![id])?;
        self.conn.execute("DELETE FROM categories WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ------------------------------------------------------------ products

    pub fn list_products(&self, f: &ProductFilter) -> Result<Vec<Product>> {
        self.require(Perm::Products)?;
        let mut sql = format!("SELECT {PRODUCT_COLS} FROM {PRODUCT_FROM} WHERE 1=1");
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if !f.include_inactive {
            sql.push_str(" AND p.active = 1");
        }
        let q = f.search.trim();
        if !q.is_empty() {
            args.push(format!("%{q}%").into());
            let n = args.len();
            sql.push_str(&format!(
                " AND (p.code LIKE ?{n} OR p.name LIKE ?{n} OR p.barcode LIKE ?{n} OR p.brand LIKE ?{n} OR p.fits_models LIKE ?{n})"
            ));
        }
        if let Some(c) = f.category_id {
            args.push(c.into());
            sql.push_str(&format!(" AND p.category_id = ?{}", args.len()));
        }
        if let Some(k) = f.kind {
            args.push(k.as_str().to_string().into());
            sql.push_str(&format!(" AND p.kind = ?{}", args.len()));
        }
        match f.level {
            LevelFilter::All => {}
            LevelFilter::Low => sql.push_str(" AND p.stock <= p.low_stock"),
            LevelFilter::Out => sql.push_str(" AND p.stock <= 0"),
        }
        if f.level == LevelFilter::Low {
            sql.push_str(" ORDER BY (p.stock * 1.0 / MAX(p.low_stock, 1)), p.name COLLATE NOCASE");
        } else {
            sql.push_str(" ORDER BY p.name COLLATE NOCASE");
        }
        if let Some(l) = f.limit {
            sql.push_str(&format!(" LIMIT {l}"));
        }
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params_from_iter(args), product_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_product(&self, id: i64) -> Result<Product> {
        Ok(self.conn.query_row(
            &format!("SELECT {PRODUCT_COLS} FROM {PRODUCT_FROM} WHERE p.id = ?1"),
            params![id],
            product_row,
        )?)
    }

    /// Exact lookup by code or barcode (used when staff type a code at the counter).
    pub fn find_by_code(&self, code: &str) -> Result<Option<Product>> {
        let code = code.trim();
        if code.is_empty() {
            return Ok(None);
        }
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {PRODUCT_COLS} FROM {PRODUCT_FROM}
                     WHERE p.active = 1 AND (p.code = ?1 COLLATE NOCASE OR p.barcode = ?1) LIMIT 1"
                ),
                params![code],
                product_row,
            )
            .optional()?)
    }

    /// Quick search for suggestions: code prefix first, then name matches.
    pub fn search_products(&self, q: &str, limit: i64) -> Result<Vec<Product>> {
        let q = q.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let mut st = self.conn.prepare(&format!(
            "SELECT {PRODUCT_COLS} FROM {PRODUCT_FROM}
             WHERE p.active = 1 AND (p.code LIKE ?1 OR p.name LIKE ?2 OR p.barcode = ?3 OR p.fits_models LIKE ?2)
             ORDER BY CASE WHEN p.code LIKE ?1 THEN 0 WHEN p.name LIKE ?1 THEN 1 ELSE 2 END, p.name COLLATE NOCASE
             LIMIT ?4"
        ))?;
        let rows = st
            .query_map(params![format!("{q}%"), format!("%{q}%"), q, limit], product_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn code_in_use(&self, code: &str, except_id: Option<i64>) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM products WHERE code = ?1 COLLATE NOCASE AND id <> ?2",
            params![clean_code(code), except_id.unwrap_or(0)],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// Next free code for a category, e.g. "BAT-004".
    pub fn suggest_code(&self, category_id: Option<i64>) -> Result<String> {
        let prefix = match category_id {
            Some(id) => {
                let (name, prefix): (String, String) = self
                    .conn
                    .query_row("SELECT name, prefix FROM categories WHERE id = ?1", params![id], |r| {
                        Ok((r.get(0)?, r.get(1)?))
                    })
                    .unwrap_or_default();
                if prefix.is_empty() {
                    auto_prefix(&name)
                } else {
                    prefix
                }
            }
            None => "P".into(),
        };
        let mut st = self.conn.prepare("SELECT code FROM products WHERE code LIKE ?1")?;
        let codes: Vec<String> =
            st.query_map(params![format!("{prefix}-%")], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        let max = codes
            .iter()
            .filter_map(|c| c.get(prefix.len() + 1..).and_then(|s| s.parse::<i64>().ok()))
            .max()
            .unwrap_or(0);
        Ok(format!("{prefix}-{:03}", max + 1))
    }

    pub fn save_product(&self, p: ProductInput) -> Result<i64> {
        self.require(Perm::EditProducts)?;
        let code = clean_code(&p.code);
        if code.is_empty() {
            return invalid("Please enter a product code.");
        }
        if p.name.trim().is_empty() {
            return invalid("Please enter the product name.");
        }
        if p.price < 0 || p.cost < 0 {
            return invalid("Prices can't be negative.");
        }
        if p.low_stock < 0 {
            return invalid("Low stock level can't be negative.");
        }
        let id = opt_id(p.id);
        if self.code_in_use(&code, id)? {
            return invalid(format!("Code {code} is already used by another product."));
        }
        let barcode = p.barcode.trim();
        let barcode: Option<&str> = if barcode.is_empty() { None } else { Some(barcode) };
        if let Some(b) = barcode {
            let n: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM products WHERE barcode = ?1 AND id <> ?2",
                params![b, id.unwrap_or(0)],
                |r| r.get(0),
            )?;
            if n > 0 {
                return invalid("This barcode is already used by another product.");
            }
        }
        let ts = now();
        let tx = self.conn.unchecked_transaction()?;
        let id = match id {
            None => {
                if p.stock < 0 {
                    return invalid("Opening stock can't be negative.");
                }
                self.conn.execute(
                    "INSERT INTO products (code, barcode, name, category_id, kind, brand, unit, cost, price, stock,
                        low_stock, fits_models, location, serial_tracked, warranty_months, active, notes, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?17)",
                    params![
                        code,
                        barcode,
                        p.name.trim(),
                        opt_id(p.category_id),
                        p.kind,
                        p.brand.trim(),
                        if p.unit.trim().is_empty() { "pcs" } else { p.unit.trim() },
                        p.cost,
                        p.price,
                        p.low_stock,
                        p.fits_models.trim(),
                        p.location.trim(),
                        p.serial_tracked,
                        p.warranty_months.max(0),
                        p.active,
                        p.notes.trim(),
                        ts
                    ],
                )?;
                let id = self.conn.last_insert_rowid();
                if p.stock > 0 {
                    self.move_stock(id, p.stock, "opening", "", p.cost, "Opening stock")?;
                }
                self.audit("product", &format!("Added {code} {}", p.name.trim()));
                id
            }
            Some(id) => {
                let old = self.get_product(id)?;
                if !self.can(Perm::ViewCost) && p.cost != old.cost {
                    return invalid("Your role can't change cost prices.");
                }
                self.conn.execute(
                    "UPDATE products SET code = ?1, barcode = ?2, name = ?3, category_id = ?4, kind = ?5, brand = ?6,
                        unit = ?7, cost = ?8, price = ?9, low_stock = ?10, fits_models = ?11, location = ?12,
                        serial_tracked = ?13, warranty_months = ?14, active = ?15, notes = ?16, updated_at = ?17
                     WHERE id = ?18",
                    params![
                        code,
                        barcode,
                        p.name.trim(),
                        opt_id(p.category_id),
                        p.kind,
                        p.brand.trim(),
                        if p.unit.trim().is_empty() { "pcs" } else { p.unit.trim() },
                        p.cost,
                        p.price,
                        p.low_stock,
                        p.fits_models.trim(),
                        p.location.trim(),
                        p.serial_tracked,
                        p.warranty_months.max(0),
                        p.active,
                        p.notes.trim(),
                        ts,
                        id
                    ],
                )?;
                if old.price != p.price {
                    self.audit(
                        "price",
                        &format!("{code}: price {} → {}", self.money().fmt(old.price), self.money().fmt(p.price)),
                    );
                }
                if old.code != code {
                    self.audit("product", &format!("Code changed {} → {code}", old.code));
                }
                id
            }
        };
        tx.commit()?;
        Ok(id)
    }

    pub fn set_product_active(&self, id: i64, active: bool) -> Result<()> {
        self.require(Perm::EditProducts)?;
        self.conn.execute(
            "UPDATE products SET active = ?1, updated_at = ?2 WHERE id = ?3",
            params![active, now(), id],
        )?;
        Ok(())
    }

    // ------------------------------------------------------------ suppliers

    pub fn list_suppliers(&self) -> Result<Vec<Supplier>> {
        let mut st =
            self.conn.prepare("SELECT id, name, phone, address, notes FROM suppliers ORDER BY name COLLATE NOCASE")?;
        let rows = st
            .query_map([], |r| {
                Ok(Supplier {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    phone: r.get(2)?,
                    address: r.get(3)?,
                    notes: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn save_supplier(&self, s: Supplier) -> Result<i64> {
        self.require(Perm::AdjustStock)?;
        if s.name.trim().is_empty() {
            return invalid("Please enter the supplier name.");
        }
        if s.id > 0 {
            self.conn.execute(
                "UPDATE suppliers SET name = ?1, phone = ?2, address = ?3, notes = ?4 WHERE id = ?5",
                params![s.name.trim(), s.phone.trim(), s.address.trim(), s.notes.trim(), s.id],
            )?;
            Ok(s.id)
        } else {
            self.conn.execute(
                "INSERT INTO suppliers (name, phone, address, notes, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![s.name.trim(), s.phone.trim(), s.address.trim(), s.notes.trim(), now()],
            )?;
            Ok(self.conn.last_insert_rowid())
        }
    }

    // ------------------------------------------------------------ workshop services

    pub fn list_services(&self, include_inactive: bool) -> Result<Vec<LaborService>> {
        let mut st = self.conn.prepare(
            "SELECT id, code, name, price, active FROM labor_services WHERE (?1 OR active = 1) ORDER BY name COLLATE NOCASE",
        )?;
        let rows = st
            .query_map(params![include_inactive], |r| {
                Ok(LaborService {
                    id: r.get(0)?,
                    code: r.get(1)?,
                    name: r.get(2)?,
                    price: r.get(3)?,
                    active: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn save_service(&self, s: LaborService) -> Result<i64> {
        self.require(Perm::EditProducts)?;
        let code = clean_code(&s.code);
        if code.is_empty() || s.name.trim().is_empty() {
            return invalid("Please enter a code and a name for the service.");
        }
        if s.price < 0 {
            return invalid("Price can't be negative.");
        }
        if s.id > 0 {
            self.conn.execute(
                "UPDATE labor_services SET code = ?1, name = ?2, price = ?3, active = ?4 WHERE id = ?5",
                params![code, s.name.trim(), s.price, s.active, s.id],
            )?;
            Ok(s.id)
        } else {
            self.conn.execute(
                "INSERT INTO labor_services (code, name, price, active, created_at) VALUES (?1, ?2, ?3, 1, ?4)",
                params![code, s.name.trim(), s.price, now()],
            )?;
            Ok(self.conn.last_insert_rowid())
        }
    }

    // ------------------------------------------------------------ vehicle models

    pub fn list_vehicle_models(&self, include_inactive: bool) -> Result<Vec<VehicleModel>> {
        let mut st = self.conn.prepare(
            "SELECT m.id, m.name, m.brand, m.price, m.cost, m.vehicle_warranty_months, m.battery_warranty_months,
                m.motor_warranty_months, m.warranty_km, m.free_services, m.active,
                (SELECT COUNT(*) FROM vehicles v WHERE v.model_id = m.id AND v.status IN ('in_stock','demo'))
             FROM vehicle_models m WHERE (?1 OR m.active = 1) ORDER BY m.name COLLATE NOCASE",
        )?;
        let rows = st.query_map(params![include_inactive], model_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_vehicle_model(&self, id: i64) -> Result<VehicleModel> {
        Ok(self.conn.query_row(
            "SELECT m.id, m.name, m.brand, m.price, m.cost, m.vehicle_warranty_months, m.battery_warranty_months,
                m.motor_warranty_months, m.warranty_km, m.free_services, m.active,
                (SELECT COUNT(*) FROM vehicles v WHERE v.model_id = m.id AND v.status IN ('in_stock','demo'))
             FROM vehicle_models m WHERE m.id = ?1",
            params![id],
            model_row,
        )?)
    }

    pub fn save_vehicle_model(&self, m: VehicleModel) -> Result<i64> {
        self.require(Perm::EditProducts)?;
        if m.name.trim().is_empty() {
            return invalid("Please enter the model name.");
        }
        if m.price < 0 || m.cost < 0 {
            return invalid("Prices can't be negative.");
        }
        if m.id > 0 {
            self.conn.execute(
                "UPDATE vehicle_models SET name = ?1, brand = ?2, price = ?3, cost = ?4, vehicle_warranty_months = ?5,
                    battery_warranty_months = ?6, motor_warranty_months = ?7, warranty_km = ?8, free_services = ?9,
                    active = ?10 WHERE id = ?11",
                params![
                    m.name.trim(),
                    m.brand.trim(),
                    m.price,
                    m.cost,
                    m.vehicle_warranty_months.max(0),
                    m.battery_warranty_months.max(0),
                    m.motor_warranty_months.max(0),
                    m.warranty_km.max(0),
                    m.free_services.max(0),
                    m.active,
                    m.id
                ],
            )?;
            self.conn.execute(
                "UPDATE vehicles SET model_name = ?1 WHERE model_id = ?2",
                params![m.name.trim(), m.id],
            )?;
            Ok(m.id)
        } else {
            self.conn.execute(
                "INSERT INTO vehicle_models (name, brand, price, cost, vehicle_warranty_months, battery_warranty_months,
                    motor_warranty_months, warranty_km, free_services, active, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10)",
                params![
                    m.name.trim(),
                    m.brand.trim(),
                    m.price,
                    m.cost,
                    m.vehicle_warranty_months.max(0),
                    m.battery_warranty_months.max(0),
                    m.motor_warranty_months.max(0),
                    m.warranty_km.max(0),
                    m.free_services.max(0),
                    now()
                ],
            )?;
            Ok(self.conn.last_insert_rowid())
        }
    }
}
