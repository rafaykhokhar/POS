//! Database connection and schema.

use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn open_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;
         PRAGMA temp_store = MEMORY;",
    )?;
    Ok(())
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch(SCHEMA_V1)?;
        conn.execute_batch("PRAGMA user_version = 1")?;
    }
    Ok(())
}

const SCHEMA_V1: &str = r#"
BEGIN;

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE counters (
    name  TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);

CREATE TABLE users (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    role       TEXT NOT NULL,
    phone      TEXT NOT NULL DEFAULT '',
    pin_hash   TEXT,
    active     INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);

CREATE TABLE categories (
    id     INTEGER PRIMARY KEY,
    name   TEXT NOT NULL UNIQUE COLLATE NOCASE,
    prefix TEXT NOT NULL DEFAULT ''
);

CREATE TABLE suppliers (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    phone      TEXT NOT NULL DEFAULT '',
    address    TEXT NOT NULL DEFAULT '',
    notes      TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

CREATE TABLE products (
    id              INTEGER PRIMARY KEY,
    code            TEXT NOT NULL UNIQUE COLLATE NOCASE,
    barcode         TEXT UNIQUE,
    name            TEXT NOT NULL,
    category_id     INTEGER REFERENCES categories(id) ON DELETE SET NULL,
    kind            TEXT NOT NULL DEFAULT 'part',
    brand           TEXT NOT NULL DEFAULT '',
    unit            TEXT NOT NULL DEFAULT 'pcs',
    cost            INTEGER NOT NULL DEFAULT 0,
    price           INTEGER NOT NULL DEFAULT 0,
    stock           INTEGER NOT NULL DEFAULT 0,
    low_stock       INTEGER NOT NULL DEFAULT 3,
    fits_models     TEXT NOT NULL DEFAULT '',
    location        TEXT NOT NULL DEFAULT '',
    serial_tracked  INTEGER NOT NULL DEFAULT 0,
    warranty_months INTEGER NOT NULL DEFAULT 0,
    active          INTEGER NOT NULL DEFAULT 1,
    notes           TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
CREATE INDEX idx_products_name ON products(name COLLATE NOCASE);
CREATE INDEX idx_products_category ON products(category_id);

CREATE TABLE customers (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    phone      TEXT NOT NULL DEFAULT '',
    id_no      TEXT NOT NULL DEFAULT '',
    address    TEXT NOT NULL DEFAULT '',
    email      TEXT NOT NULL DEFAULT '',
    notes      TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
CREATE INDEX idx_customers_phone ON customers(phone);
CREATE INDEX idx_customers_name ON customers(name COLLATE NOCASE);

CREATE TABLE labor_services (
    id         INTEGER PRIMARY KEY,
    code       TEXT NOT NULL UNIQUE COLLATE NOCASE,
    name       TEXT NOT NULL,
    price      INTEGER NOT NULL DEFAULT 0,
    active     INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL
);

CREATE TABLE vehicle_models (
    id                      INTEGER PRIMARY KEY,
    name                    TEXT NOT NULL UNIQUE COLLATE NOCASE,
    brand                   TEXT NOT NULL DEFAULT '',
    price                   INTEGER NOT NULL DEFAULT 0,
    cost                    INTEGER NOT NULL DEFAULT 0,
    vehicle_warranty_months INTEGER NOT NULL DEFAULT 0,
    battery_warranty_months INTEGER NOT NULL DEFAULT 0,
    motor_warranty_months   INTEGER NOT NULL DEFAULT 0,
    warranty_km             INTEGER NOT NULL DEFAULT 0,
    free_services           INTEGER NOT NULL DEFAULT 0,
    active                  INTEGER NOT NULL DEFAULT 1,
    created_at              TEXT NOT NULL
);

CREATE TABLE vehicles (
    id                     INTEGER PRIMARY KEY,
    model_id               INTEGER REFERENCES vehicle_models(id),
    model_name             TEXT NOT NULL,
    chassis_no             TEXT UNIQUE COLLATE NOCASE,
    motor_no               TEXT NOT NULL DEFAULT '',
    battery_serial         TEXT NOT NULL DEFAULT '',
    color                  TEXT NOT NULL DEFAULT '',
    year                   INTEGER NOT NULL DEFAULT 0,
    cost                   INTEGER NOT NULL DEFAULT 0,
    price                  INTEGER NOT NULL DEFAULT 0,
    status                 TEXT NOT NULL,
    customer_id            INTEGER REFERENCES customers(id),
    registration_no        TEXT NOT NULL DEFAULT '',
    sold_at                TEXT NOT NULL DEFAULT '',
    sale_id                INTEGER,
    vehicle_warranty_until TEXT NOT NULL DEFAULT '',
    battery_warranty_until TEXT NOT NULL DEFAULT '',
    motor_warranty_until   TEXT NOT NULL DEFAULT '',
    warranty_km            INTEGER NOT NULL DEFAULT 0,
    notes                  TEXT NOT NULL DEFAULT '',
    received_at            TEXT NOT NULL DEFAULT '',
    created_at             TEXT NOT NULL
);
CREATE INDEX idx_vehicles_status ON vehicles(status);
CREATE INDEX idx_vehicles_customer ON vehicles(customer_id);
CREATE INDEX idx_vehicles_reg ON vehicles(registration_no COLLATE NOCASE);

CREATE TABLE service_coupons (
    id         INTEGER PRIMARY KEY,
    vehicle_id INTEGER NOT NULL REFERENCES vehicles(id),
    seq        INTEGER NOT NULL,
    used       INTEGER NOT NULL DEFAULT 0,
    job_id     INTEGER,
    used_at    TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_coupons_vehicle ON service_coupons(vehicle_id);

CREATE TABLE bookings (
    id            INTEGER PRIMARY KEY,
    booking_no    TEXT NOT NULL UNIQUE,
    customer_id   INTEGER NOT NULL REFERENCES customers(id),
    model_id      INTEGER REFERENCES vehicle_models(id),
    vehicle_id    INTEGER REFERENCES vehicles(id),
    color         TEXT NOT NULL DEFAULT '',
    agreed_price  INTEGER NOT NULL DEFAULT 0,
    status        TEXT NOT NULL DEFAULT 'open',
    expected_date TEXT NOT NULL DEFAULT '',
    sale_id       INTEGER,
    notes         TEXT NOT NULL DEFAULT '',
    user_id       INTEGER,
    created_at    TEXT NOT NULL
);

CREATE TABLE job_cards (
    id                INTEGER PRIMARY KEY,
    job_no            TEXT NOT NULL UNIQUE,
    vehicle_id        INTEGER NOT NULL REFERENCES vehicles(id),
    customer_id       INTEGER REFERENCES customers(id),
    odometer          INTEGER NOT NULL DEFAULT 0,
    battery_note      TEXT NOT NULL DEFAULT '',
    complaint         TEXT NOT NULL DEFAULT '',
    diagnosis         TEXT NOT NULL DEFAULT '',
    status            TEXT NOT NULL,
    technician_id     INTEGER REFERENCES users(id),
    advisor_id        INTEGER REFERENCES users(id),
    is_warranty       INTEGER NOT NULL DEFAULT 0,
    coupon_id         INTEGER,
    approved          INTEGER NOT NULL DEFAULT 0,
    next_service_date TEXT NOT NULL DEFAULT '',
    next_service_km   INTEGER NOT NULL DEFAULT 0,
    sale_id           INTEGER,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    delivered_at      TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_jobs_status ON job_cards(status);
CREATE INDEX idx_jobs_vehicle ON job_cards(vehicle_id);

CREATE TABLE job_items (
    id          INTEGER PRIMARY KEY,
    job_id      INTEGER NOT NULL REFERENCES job_cards(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,
    product_id  INTEGER REFERENCES products(id),
    service_id  INTEGER REFERENCES labor_services(id),
    code        TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL,
    qty         INTEGER NOT NULL DEFAULT 1,
    unit_price  INTEGER NOT NULL DEFAULT 0,
    unit_cost   INTEGER NOT NULL DEFAULT 0,
    free        INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_job_items_job ON job_items(job_id);

CREATE TABLE sales (
    id               INTEGER PRIMARY KEY,
    invoice_no       TEXT NOT NULL UNIQUE,
    kind             TEXT NOT NULL,
    status           TEXT NOT NULL DEFAULT 'completed',
    customer_id      INTEGER REFERENCES customers(id),
    user_id          INTEGER REFERENCES users(id),
    salesperson_id   INTEGER REFERENCES users(id),
    subtotal         INTEGER NOT NULL DEFAULT 0,
    discount         INTEGER NOT NULL DEFAULT 0,
    tax              INTEGER NOT NULL DEFAULT 0,
    total            INTEGER NOT NULL DEFAULT 0,
    original_sale_id INTEGER REFERENCES sales(id),
    job_id           INTEGER REFERENCES job_cards(id),
    booking_id       INTEGER REFERENCES bookings(id),
    notes            TEXT NOT NULL DEFAULT '',
    created_at       TEXT NOT NULL
);
CREATE INDEX idx_sales_created ON sales(created_at);
CREATE INDEX idx_sales_customer ON sales(customer_id);

CREATE TABLE sale_items (
    id             INTEGER PRIMARY KEY,
    sale_id        INTEGER NOT NULL REFERENCES sales(id) ON DELETE CASCADE,
    item_type      TEXT NOT NULL,
    product_id     INTEGER REFERENCES products(id),
    vehicle_id     INTEGER REFERENCES vehicles(id),
    service_id     INTEGER REFERENCES labor_services(id),
    section        TEXT NOT NULL,
    group_key      TEXT NOT NULL,
    item_name      TEXT NOT NULL,
    code           TEXT NOT NULL DEFAULT '',
    description    TEXT NOT NULL,
    qty            INTEGER NOT NULL,
    unit_price     INTEGER NOT NULL,
    unit_cost      INTEGER NOT NULL DEFAULT 0,
    line_discount  INTEGER NOT NULL DEFAULT 0,
    net_amount     INTEGER NOT NULL DEFAULT 0,
    cost_amount    INTEGER NOT NULL DEFAULT 0,
    free           INTEGER NOT NULL DEFAULT 0,
    serial_no      TEXT NOT NULL DEFAULT '',
    warranty_until TEXT NOT NULL DEFAULT '',
    ref_item_id    INTEGER REFERENCES sale_items(id)
);
CREATE INDEX idx_sale_items_sale ON sale_items(sale_id);
CREATE INDEX idx_sale_items_ref ON sale_items(ref_item_id);

CREATE TABLE shifts (
    id           INTEGER PRIMARY KEY,
    opened_by    INTEGER REFERENCES users(id),
    opened_at    TEXT NOT NULL,
    opening_cash INTEGER NOT NULL DEFAULT 0,
    closed_by    INTEGER REFERENCES users(id),
    closed_at    TEXT NOT NULL DEFAULT '',
    counted_cash INTEGER,
    expected_cash INTEGER,
    notes        TEXT NOT NULL DEFAULT ''
);

CREATE TABLE payments (
    id          INTEGER PRIMARY KEY,
    sale_id     INTEGER REFERENCES sales(id),
    booking_id  INTEGER REFERENCES bookings(id),
    customer_id INTEGER REFERENCES customers(id),
    method      TEXT NOT NULL,
    amount      INTEGER NOT NULL,
    bank_name   TEXT NOT NULL DEFAULT '',
    reference   TEXT NOT NULL DEFAULT '',
    note        TEXT NOT NULL DEFAULT '',
    user_id     INTEGER REFERENCES users(id),
    shift_id    INTEGER REFERENCES shifts(id),
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_payments_sale ON payments(sale_id);
CREATE INDEX idx_payments_booking ON payments(booking_id);
CREATE INDEX idx_payments_created ON payments(created_at);

CREATE TABLE cash_moves (
    id         INTEGER PRIMARY KEY,
    shift_id   INTEGER NOT NULL REFERENCES shifts(id),
    amount     INTEGER NOT NULL,
    reason     TEXT NOT NULL DEFAULT '',
    user_id    INTEGER REFERENCES users(id),
    created_at TEXT NOT NULL
);

CREATE TABLE purchases (
    id          INTEGER PRIMARY KEY,
    po_no       TEXT NOT NULL UNIQUE,
    supplier_id INTEGER REFERENCES suppliers(id),
    total       INTEGER NOT NULL DEFAULT 0,
    notes       TEXT NOT NULL DEFAULT '',
    user_id     INTEGER REFERENCES users(id),
    created_at  TEXT NOT NULL
);

CREATE TABLE purchase_items (
    id          INTEGER PRIMARY KEY,
    purchase_id INTEGER NOT NULL REFERENCES purchases(id) ON DELETE CASCADE,
    product_id  INTEGER NOT NULL REFERENCES products(id),
    qty         INTEGER NOT NULL,
    unit_cost   INTEGER NOT NULL
);

CREATE TABLE stock_movements (
    id         INTEGER PRIMARY KEY,
    product_id INTEGER NOT NULL REFERENCES products(id),
    change     INTEGER NOT NULL,
    reason     TEXT NOT NULL,
    reference  TEXT NOT NULL DEFAULT '',
    note       TEXT NOT NULL DEFAULT '',
    unit_cost  INTEGER NOT NULL DEFAULT 0,
    user_id    INTEGER REFERENCES users(id),
    created_at TEXT NOT NULL
);
CREATE INDEX idx_moves_product ON stock_movements(product_id);

CREATE TABLE audit_log (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER REFERENCES users(id),
    action     TEXT NOT NULL,
    detail     TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
CREATE INDEX idx_audit_created ON audit_log(created_at);

COMMIT;
"#;
