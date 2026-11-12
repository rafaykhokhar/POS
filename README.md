# EV Showroom POS

A fast, offline point-of-sale desktop app for an electric-vehicle showroom: **bike sales, workshop service and spare parts** in one place. Written in **Rust** with a native **Slint** UI and an embedded **SQLite** database.

## What's inside

| Area | What it does |
| --- | --- |
| **Dashboard** | Today's sales & profit, 7-day chart, low stock, workshop board summary, services due, recent sales |
| **Counter** | Type a product code (or `2*CODE`) and press Enter, name search with ↑/↓, per-item and bill discounts, hold/resume bills, cash / bank / split payment with change, receipts |
| **Vehicles** | Every bike tracked by chassis, motor and battery number; status (in stock, booked, demo, in transit, sold); sell with accessories; vehicle/battery/motor warranty and free-service coupons created automatically; bookings with advance payments; models |
| **Workshop** | Job-card board (Received → Diagnosing → Waiting for parts → In progress → Ready); parts are taken from stock when added; labour list; warranty and free-coupon jobs; deliver & bill; next-service reminders; full service history per bike |
| **Parts & Stock** | Staff-entered codes (with optional suggestions), categories, **red when at/below the low-stock level, amber when close**, stock adjustments with reasons, stocktake, receive stock from suppliers (average cost updates), stock history |
| **Customers** | Profiles with their bikes, purchases, workshop visits and payments; balances owed; receive balance payments |
| **Reports** | Automatic **profit sheet** (from cost and sale price at the moment of each sale) by section and by item, day-by-day chart, sales list with full bill details and **returns/refunds**, stock value; export to Excel (CSV) |
| **Cash Drawer** | Open with a float, cash in/out with reasons, count & close with the difference shown |
| **Settings** | Shop details, currency & tax (inclusive or added), staff with roles and PINs, workshop services, backups, activity log |
| **Printing** | 80 mm thermal receipts, A4 invoices (with chassis & warranty details for bikes) and job cards. They open in the system print dialog, so any installed printer works, or save as PDF |

Roles: Owner, Manager, Salesperson, Service Advisor, Technician, Cashier. Each sees only what they're allowed to.

## Run it (macOS, for development)

```bash
cargo run -p pos-app
```

The first launch shows a setup screen (shop name, currency, tax, owner PIN). Tick **"Fill with sample data"** to explore with example bikes, parts, customers and jobs.

## Build the release version

```bash
cargo build --release -p pos-app
```

The app is at `target/release/ev-pos` (on Windows `target\release\ev-pos.exe`).

### Windows

On a Windows PC with [Rust](https://rustup.rs) installed (choose the default MSVC option; it installs the Visual Studio C++ build tools it needs):

```powershell
cargo build --release -p pos-app
```

No console window is shown in release builds. The GitHub Actions workflow in `.github/workflows/build.yml` also builds the Windows `.exe` automatically when the code is pushed to GitHub, so you don't need a Windows PC to compile, only to test.

## Where the data lives

One database file, created on first run:

- **macOS:** `~/Library/Application Support/EV Showroom POS/pos.db`
- **Windows:** `%APPDATA%\EV Showroom POS\pos.db`

A backup is written to the `backups` folder next to it every day when the app closes (the newest 30 are kept). **Settings → Backup & data → Back up now** saves a copy anywhere you like.

To start a fresh shop, close the app and delete (or rename) `pos.db`.

## Project layout

```
crates/pos-core   Business logic + SQLite (no UI). All rules, money maths, permissions, reports.
crates/pos-app    Desktop app: Slint screens (ui/) and the Rust code that connects them (src/screens/).
```

Money is stored as whole minor units (no floating point), so totals and profit never drift.

## Tests

```bash
cargo test
```

Covers sales, stock levels, discounts, tax (inclusive and exclusive), average cost, returns, vehicle sales with warranty and coupons, bookings, the workshop flow, permissions, the cash drawer, and printing.

## Licence note

The UI uses [Slint](https://slint.dev) under its royalty-free desktop licence, which asks for the "Made with Slint" credit shown on **Settings → About**.
