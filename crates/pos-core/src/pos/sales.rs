use super::{opt_id, Pos};
use crate::error::{invalid, Error, Result};
use crate::models::*;
use crate::money::{mul_bp, round_div, Money};
use crate::perms::Perm;
use crate::util::{add_months, day_after, day_start, now};
use rusqlite::{params, params_from_iter, Row};

/// Bill totals. The same maths is used by the counter screen and when saving.
pub fn compute_totals(lines: &[CartLine], discount: Money, tax_bp: i64, inclusive: bool) -> Totals {
    let mut gross = 0;
    let mut line_disc = 0;
    let mut items = 0;
    for l in lines {
        let g = l.gross();
        gross += g;
        line_disc += g - l.net();
        items += l.qty;
    }
    let after_lines = gross - line_disc;
    let discount = discount.clamp(0, after_lines.max(0));
    let base = after_lines - discount;
    let (tax, total) = if tax_bp <= 0 {
        (0, base)
    } else if inclusive {
        (round_div(base as i128 * tax_bp as i128, (10_000 + tax_bp) as i128), base)
    } else {
        let t = mul_bp(base, tax_bp);
        (t, base + t)
    };
    Totals { gross, line_discounts: line_disc, discount, tax, total, items }
}

/// Split the bill's revenue (after all discounts, excluding tax) across lines,
/// so profit per item is exact. Rounding leftovers go to the biggest line.
fn allocate_net(lines: &[CartLine], totals: &Totals) -> Vec<Money> {
    let net_total = totals.total - totals.tax;
    let nets: Vec<Money> = lines.iter().map(|l| l.net()).collect();
    let sum: Money = nets.iter().sum();
    if sum == 0 {
        return vec![0; lines.len()];
    }
    let mut out: Vec<Money> =
        nets.iter().map(|n| round_div(*n as i128 * net_total as i128, sum as i128)).collect();
    let diff = net_total - out.iter().sum::<Money>();
    if diff != 0 {
        let idx = nets.iter().enumerate().max_by_key(|(_, n)| **n).map(|(i, _)| i).unwrap_or(0);
        out[idx] += diff;
    }
    out
}

pub(crate) const SALE_COLS: &str = "s.id, s.invoice_no, s.kind, s.status, s.customer_id, COALESCE(c.name, ''),
    COALESCE(u.name, ''), s.total,
    (SELECT COALESCE(SUM(amount), 0) FROM payments WHERE sale_id = s.id),
    (SELECT COALESCE(SUM(net_amount - cost_amount), 0) FROM sale_items WHERE sale_id = s.id),
    (SELECT COALESCE(SUM(qty), 0) FROM sale_items WHERE sale_id = s.id),
    s.created_at";

pub(crate) const SALE_FROM: &str =
    "sales s LEFT JOIN customers c ON c.id = s.customer_id LEFT JOIN users u ON u.id = s.user_id";

pub(crate) fn sale_row(r: &Row) -> rusqlite::Result<SaleSummary> {
    Ok(SaleSummary {
        id: r.get(0)?,
        invoice_no: r.get(1)?,
        kind: r.get(2)?,
        status: r.get(3)?,
        customer_id: r.get(4)?,
        customer_name: r.get(5)?,
        user_name: r.get(6)?,
        total: r.get(7)?,
        paid: r.get(8)?,
        profit: r.get(9)?,
        items: r.get(10)?,
        created_at: r.get(11)?,
    })
}

pub(crate) const PAYMENT_COLS: &str = "p.id, p.sale_id, p.booking_id,
    COALESCE(s.invoice_no, b.booking_no, ''), p.method, p.amount, p.bank_name, p.reference, p.note,
    COALESCE(u.name, ''), p.created_at";

pub(crate) const PAYMENT_FROM: &str = "payments p LEFT JOIN sales s ON s.id = p.sale_id
    LEFT JOIN bookings b ON b.id = p.booking_id LEFT JOIN users u ON u.id = p.user_id";

pub(crate) fn payment_row(r: &Row) -> rusqlite::Result<Payment> {
    Ok(Payment {
        id: r.get(0)?,
        sale_id: r.get(1)?,
        booking_id: r.get(2)?,
        reference_no: r.get(3)?,
        method: r.get(4)?,
        amount: r.get(5)?,
        bank_name: r.get(6)?,
        reference: r.get(7)?,
        note: r.get(8)?,
        user_name: r.get(9)?,
        created_at: r.get(10)?,
    })
}

/// Extra options used internally (workshop delivery, demo data).
#[derive(Default)]
pub(crate) struct SaleOpts {
    pub ts: Option<String>,
    /// Unit costs already fixed (workshop parts were taken from stock when added to the job).
    pub preset_costs: Option<Vec<Money>>,
    pub job_id: Option<i64>,
}

struct Resolved {
    section: Section,
    group_key: String,
    item_name: String,
    unit_cost: Money,
    warranty_until: String,
    product_id: Option<i64>,
    vehicle: Option<Vehicle>,
    service_id: Option<i64>,
}

impl Pos {
    /// Totals using the shop's tax settings.
    pub fn totals(&self, lines: &[CartLine], discount: Money) -> Totals {
        let s = self.settings.borrow();
        compute_totals(lines, discount, s.tax_rate_bp, s.tax_inclusive)
    }

    /// Complete a sale: saves the bill, takes items out of stock and records payments.
    pub fn checkout(&self, input: CheckoutInput) -> Result<SaleReceipt> {
        let tx = self.conn.unchecked_transaction()?;
        let r = self.checkout_inner(&input, SaleOpts::default())?;
        tx.commit()?;
        Ok(r)
    }

    pub(crate) fn checkout_inner(&self, input: &CheckoutInput, opts: SaleOpts) -> Result<SaleReceipt> {
        let perm = match input.kind {
            SaleKind::Counter => Perm::Counter,
            SaleKind::Vehicle => Perm::SellVehicles,
            SaleKind::Workshop => Perm::Workshop,
            SaleKind::Return => return invalid("Use the return screen for returns."),
        };
        self.require(perm)?;
        if input.items.is_empty() {
            return invalid("The bill is empty.");
        }
        for l in &input.items {
            if l.qty <= 0 {
                return invalid("Quantity must be at least 1.");
            }
            if l.unit_price < 0 || l.line_discount < 0 {
                return invalid("Prices and discounts can't be negative.");
            }
        }
        let ts = opts.ts.clone().unwrap_or_else(now);
        let date = ts[..10].to_string();
        let settings = self.settings();
        let totals = compute_totals(&input.items, input.discount, settings.tax_rate_bp, settings.tax_inclusive);
        let can_discount = self.can(Perm::Discounts);
        if (totals.discount > 0 || totals.line_discounts > 0) && !can_discount && opts.preset_costs.is_none() {
            return Err(Error::Denied("Your role can't give discounts. Ask a manager.".into()));
        }
        let customer_id = opt_id(input.customer_id);
        let booking_id = opt_id(input.booking_id);

        // ---- payments
        if input.payments.iter().any(|p| p.amount < 0) {
            return invalid("Payment amounts can't be negative.");
        }
        let new_paid: Money = input.payments.iter().map(|p| p.amount).sum();
        let advance: Money = match booking_id {
            Some(b) => self.conn.query_row(
                "SELECT COALESCE(SUM(amount), 0) FROM payments WHERE booking_id = ?1 AND sale_id IS NULL",
                params![b],
                |r| r.get(0),
            )?,
            None => 0,
        };
        let paid = new_paid + advance;
        if paid > totals.total {
            return invalid("Payment is more than the bill total.");
        }
        if paid < totals.total && customer_id.is_none() {
            return invalid("Select a customer to leave part of the bill unpaid.");
        }
        let has_vehicle = input.items.iter().any(|l| l.item_type == ItemType::Vehicle);
        if has_vehicle && customer_id.is_none() {
            return invalid("Select the customer who is buying the vehicle.");
        }
        let booking_vehicle: Option<i64> = match booking_id {
            Some(b) => {
                let (status, vid): (BookingStatus, Option<i64>) = self.conn.query_row(
                    "SELECT status, vehicle_id FROM bookings WHERE id = ?1",
                    params![b],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                if status != BookingStatus::Open {
                    return invalid("This booking is no longer open.");
                }
                vid
            }
            None => None,
        };

        // ---- resolve every line
        let mut resolved = Vec::with_capacity(input.items.len());
        for (i, l) in input.items.iter().enumerate() {
            let preset = opts.preset_costs.as_ref().and_then(|c| c.get(i).copied());
            let r = match l.item_type {
                ItemType::Product => {
                    let pid = l.ref_id.ok_or_else(|| Error::Invalid("Product missing.".into()))?;
                    let p = self.get_product(pid)?;
                    if preset.is_none() {
                        if !p.active {
                            return invalid(format!("{} is no longer sold.", p.name));
                        }
                        let wanted: i64 = input
                            .items
                            .iter()
                            .filter(|x| x.item_type == ItemType::Product && x.ref_id == Some(pid))
                            .map(|x| x.qty)
                            .sum();
                        if p.stock < wanted && !settings.allow_negative_stock {
                            return invalid(format!(
                                "Only {} of {} ({}) in stock.",
                                p.stock.max(0),
                                p.name,
                                p.code
                            ));
                        }
                        if l.unit_price < p.price && !can_discount {
                            return Err(Error::Denied(
                                "Your role can't sell below the set price. Ask a manager.".into(),
                            ));
                        }
                    }
                    Resolved {
                        section: if input.kind == SaleKind::Workshop { Section::WorkshopParts } else { Section::Part },
                        group_key: format!("p:{}", p.id),
                        item_name: p.name.clone(),
                        unit_cost: preset.unwrap_or(p.cost),
                        warranty_until: add_months(&date, p.warranty_months),
                        product_id: Some(p.id),
                        vehicle: None,
                        service_id: None,
                    }
                }
                ItemType::Vehicle => {
                    let vid = l.ref_id.ok_or_else(|| Error::Invalid("Vehicle missing.".into()))?;
                    let v = self.get_vehicle(vid)?;
                    if l.qty != 1 {
                        return invalid("A vehicle line must have quantity 1.");
                    }
                    match v.status {
                        VehicleStatus::InStock | VehicleStatus::Demo => {
                            if let Some(bv) = booking_vehicle {
                                if bv != v.id {
                                    return invalid("This booking is for a different bike.");
                                }
                            }
                        }
                        VehicleStatus::Booked => {
                            if booking_vehicle != Some(v.id) {
                                return invalid(format!(
                                    "Bike {} is booked for another customer.",
                                    v.chassis_no
                                ));
                            }
                        }
                        VehicleStatus::Sold => return invalid(format!("Bike {} is already sold.", v.chassis_no)),
                        VehicleStatus::InTransit => {
                            return invalid(format!("Bike {} hasn't arrived yet (in transit).", v.chassis_no))
                        }
                        VehicleStatus::External => return invalid("This is a customer's bike, not shop stock."),
                    }
                    Resolved {
                        section: Section::Vehicle,
                        group_key: format!("m:{}", v.model_name.to_lowercase()),
                        item_name: v.model_name.clone(),
                        unit_cost: v.cost,
                        warranty_until: String::new(),
                        product_id: None,
                        vehicle: Some(v),
                        service_id: None,
                    }
                }
                ItemType::Service => Resolved {
                    section: Section::Labor,
                    group_key: match l.ref_id {
                        Some(id) => format!("s:{id}"),
                        None => format!("c:{}", l.description.to_lowercase()),
                    },
                    item_name: l.description.clone(),
                    unit_cost: preset.unwrap_or(0),
                    warranty_until: String::new(),
                    product_id: None,
                    vehicle: None,
                    service_id: opt_id(l.ref_id),
                },
                ItemType::Custom => Resolved {
                    section: Section::Other,
                    group_key: format!("c:{}", l.description.to_lowercase()),
                    item_name: l.description.clone(),
                    unit_cost: preset.unwrap_or(0),
                    warranty_until: String::new(),
                    product_id: None,
                    vehicle: None,
                    service_id: None,
                },
            };
            resolved.push(r);
        }

        let nets = allocate_net(&input.items, &totals);
        let invoice_no = self.next_no("INV")?;
        self.conn.execute(
            "INSERT INTO sales (invoice_no, kind, status, customer_id, user_id, salesperson_id, subtotal, discount, tax,
                total, job_id, booking_id, notes, created_at)
             VALUES (?1, ?2, 'completed', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                invoice_no,
                input.kind,
                customer_id,
                self.uid(),
                opt_id(input.salesperson_id).or(self.uid()),
                totals.gross - totals.line_discounts,
                totals.discount,
                totals.tax,
                totals.total,
                opts.job_id,
                booking_id,
                input.notes.trim(),
                ts
            ],
        )?;
        let sale_id = self.conn.last_insert_rowid();

        for ((l, r), net) in input.items.iter().zip(resolved.iter()).zip(nets.iter()) {
            self.conn.execute(
                "INSERT INTO sale_items (sale_id, item_type, product_id, vehicle_id, service_id, section, group_key,
                    item_name, code, description, qty, unit_price, unit_cost, line_discount, net_amount, cost_amount,
                    free, serial_no, warranty_until)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
                params![
                    sale_id,
                    l.item_type,
                    r.product_id,
                    r.vehicle.as_ref().map(|v| v.id),
                    r.service_id,
                    r.section,
                    r.group_key,
                    r.item_name,
                    l.code.trim(),
                    l.description.trim(),
                    l.qty,
                    l.unit_price,
                    r.unit_cost,
                    l.gross() - l.net(),
                    net,
                    r.unit_cost * l.qty,
                    l.free,
                    l.serial_no.trim(),
                    r.warranty_until
                ],
            )?;
            if let Some(pid) = r.product_id {
                if opts.preset_costs.is_none() {
                    self.move_stock_at(pid, -l.qty, "sale", &invoice_no, r.unit_cost, "", &ts)?;
                }
            }
            if let (Some(v), Some(cid)) = (&r.vehicle, customer_id) {
                self.mark_vehicle_sold(v, cid, sale_id, &ts)?;
            }
        }

        for p in input.payments.iter().filter(|p| p.amount > 0) {
            self.insert_payment(Some(sale_id), None, customer_id, p, &ts)?;
        }
        if let Some(b) = booking_id {
            self.conn.execute(
                "UPDATE payments SET sale_id = ?1 WHERE booking_id = ?2 AND sale_id IS NULL",
                params![sale_id, b],
            )?;
            self.conn.execute(
                "UPDATE bookings SET status = 'delivered', sale_id = ?1 WHERE id = ?2",
                params![sale_id, b],
            )?;
        }
        self.audit("sale", &format!("{invoice_no} {}", self.money().fmt(totals.total)));
        Ok(SaleReceipt { sale_id, invoice_no, total: totals.total, paid, balance: totals.total - paid })
    }

    pub(crate) fn insert_payment(
        &self,
        sale_id: Option<i64>,
        booking_id: Option<i64>,
        customer_id: Option<i64>,
        p: &PaymentInput,
        ts: &str,
    ) -> Result<()> {
        let shift = if p.method == PayMethod::Cash { self.open_shift_id()? } else { None };
        self.conn.execute(
            "INSERT INTO payments (sale_id, booking_id, customer_id, method, amount, bank_name, reference, note,
                user_id, shift_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                sale_id,
                booking_id,
                customer_id,
                p.method,
                p.amount,
                p.bank_name.trim(),
                p.reference.trim(),
                p.note.trim(),
                self.uid(),
                shift,
                ts
            ],
        )?;
        Ok(())
    }

    pub fn list_sales(&self, f: &SalesFilter) -> Result<Vec<SaleSummary>> {
        self.require(Perm::Counter).or_else(|_| self.require(Perm::Reports))?;
        let mut sql = format!("SELECT {SALE_COLS} FROM {SALE_FROM} WHERE 1=1");
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if !f.from.is_empty() {
            args.push(day_start(&f.from).into());
            sql.push_str(&format!(" AND s.created_at >= ?{}", args.len()));
        }
        if !f.to.is_empty() {
            args.push(day_after(&f.to).into());
            sql.push_str(&format!(" AND s.created_at < ?{}", args.len()));
        }
        if let Some(k) = f.kind {
            args.push(k.as_str().to_string().into());
            sql.push_str(&format!(" AND s.kind = ?{}", args.len()));
        }
        if let Some(c) = f.customer_id {
            args.push(c.into());
            sql.push_str(&format!(" AND s.customer_id = ?{}", args.len()));
        }
        let q = f.search.trim();
        if !q.is_empty() {
            args.push(format!("%{q}%").into());
            let n = args.len();
            sql.push_str(&format!(
                " AND (s.invoice_no LIKE ?{n} OR c.name LIKE ?{n} OR c.phone LIKE ?{n}
                   OR EXISTS (SELECT 1 FROM sale_items si WHERE si.sale_id = s.id AND (si.code LIKE ?{n} OR si.description LIKE ?{n})))"
            ));
        }
        sql.push_str(" ORDER BY s.id DESC");
        sql.push_str(&format!(" LIMIT {}", f.limit.unwrap_or(500)));
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params_from_iter(args), sale_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_sale(&self, id: i64) -> Result<SaleDetail> {
        let sale = self.conn.query_row(&format!("SELECT {SALE_COLS} FROM {SALE_FROM} WHERE s.id = ?1"), params![id], sale_row)?;
        let (subtotal, discount, tax, notes, sp, phone, orig): (Money, Money, Money, String, String, String, String) =
            self.conn.query_row(
                "SELECT s.subtotal, s.discount, s.tax, s.notes, COALESCE(sp.name, ''), COALESCE(c.phone, ''),
                    COALESCE(o.invoice_no, '')
                 FROM sales s LEFT JOIN users sp ON sp.id = s.salesperson_id
                 LEFT JOIN customers c ON c.id = s.customer_id LEFT JOIN sales o ON o.id = s.original_sale_id
                 WHERE s.id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
            )?;
        let mut st = self.conn.prepare(
            "SELECT si.id, si.item_type, si.product_id, si.vehicle_id, si.code, si.description, si.qty, si.unit_price,
                si.line_discount, si.net_amount, si.cost_amount, si.free, si.serial_no, si.warranty_until,
                (SELECT COALESCE(-SUM(r.qty), 0) FROM sale_items r WHERE r.ref_item_id = si.id)
             FROM sale_items si WHERE si.sale_id = ?1 ORDER BY si.id",
        )?;
        let items = st
            .query_map(params![id], |r| {
                Ok(SaleItem {
                    id: r.get(0)?,
                    item_type: r.get(1)?,
                    product_id: r.get(2)?,
                    vehicle_id: r.get(3)?,
                    code: r.get(4)?,
                    description: r.get(5)?,
                    qty: r.get(6)?,
                    unit_price: r.get(7)?,
                    line_discount: r.get(8)?,
                    net_amount: r.get(9)?,
                    cost_amount: r.get(10)?,
                    free: r.get(11)?,
                    serial_no: r.get(12)?,
                    warranty_until: r.get(13)?,
                    returned_qty: r.get(14)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut st =
            self.conn.prepare(&format!("SELECT {PAYMENT_COLS} FROM {PAYMENT_FROM} WHERE p.sale_id = ?1 ORDER BY p.id"))?;
        let payments = st.query_map(params![id], payment_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(SaleDetail {
            sale,
            subtotal,
            discount,
            tax,
            notes,
            salesperson_name: sp,
            customer_phone: phone,
            original_invoice: orig,
            items,
            payments,
        })
    }

    /// Return items from a sale. Products go back into stock.
    /// `refund`: how the money is given back (`None` = keep as customer credit).
    pub fn return_items(
        &self,
        sale_id: i64,
        lines: &[(i64, i64)],
        refund: Option<PayMethod>,
        note: &str,
    ) -> Result<SaleReceipt> {
        self.require(Perm::Refunds)?;
        let orig = self.get_sale(sale_id)?;
        if orig.sale.kind == SaleKind::Return {
            return invalid("You can't return a return.");
        }
        if orig.sale.status != SaleStatus::Completed {
            return invalid("This sale was voided.");
        }
        let lines: Vec<(i64, i64)> = lines.iter().copied().filter(|(_, q)| *q > 0).collect();
        if lines.is_empty() {
            return invalid("Choose at least one item to return.");
        }
        if refund.is_none() && orig.sale.customer_id.is_none() {
            return invalid("Choose a refund method (this sale has no customer to keep credit for).");
        }
        let orig_net: Money = orig.items.iter().map(|i| i.net_amount).sum();
        let ts = now();
        let tx = self.conn.unchecked_transaction()?;
        let ret_no = self.next_no("RET")?;

        struct RetLine<'a> {
            item: &'a SaleItem,
            qty: i64,
            net: Money,
            cost: Money,
        }
        let mut rl = Vec::new();
        for (item_id, qty) in &lines {
            let item = orig
                .items
                .iter()
                .find(|i| i.id == *item_id)
                .ok_or_else(|| Error::Invalid("Item not on this bill.".into()))?;
            if item.item_type == ItemType::Vehicle {
                return invalid("Vehicle sales can't be returned here. Please contact the owner.");
            }
            let left = item.qty - item.returned_qty;
            if *qty > left {
                return invalid(format!("Only {left} of {} can still be returned.", item.description));
            }
            let net = round_div(item.net_amount as i128 * *qty as i128, item.qty as i128);
            let cost = round_div(item.cost_amount as i128 * *qty as i128, item.qty as i128);
            rl.push(RetLine { item, qty: *qty, net, cost });
        }
        let net_sum: Money = rl.iter().map(|l| l.net).sum();
        // Refund includes the tax share that was charged on these items.
        let refund_total = if orig_net > 0 {
            round_div(net_sum as i128 * orig.sale.total as i128, orig_net as i128)
        } else {
            net_sum
        };
        let tax_back = refund_total - net_sum;

        self.conn.execute(
            "INSERT INTO sales (invoice_no, kind, status, customer_id, user_id, salesperson_id, subtotal, discount, tax,
                total, original_sale_id, notes, created_at)
             VALUES (?1, 'return', 'completed', ?2, ?3, ?3, ?4, 0, ?5, ?6, ?7, ?8, ?9)",
            params![
                ret_no,
                orig.sale.customer_id,
                self.uid(),
                -net_sum,
                -tax_back,
                -refund_total,
                sale_id,
                note.trim(),
                ts
            ],
        )?;
        let ret_id = self.conn.last_insert_rowid();
        for l in &rl {
            let (section, group_key, item_name, service_id): (String, String, String, Option<i64>) = self.conn.query_row(
                "SELECT section, group_key, item_name, service_id FROM sale_items WHERE id = ?1",
                params![l.item.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
            let unit_cost = if l.qty > 0 { l.cost / l.qty } else { 0 };
            self.conn.execute(
                "INSERT INTO sale_items (sale_id, item_type, product_id, vehicle_id, service_id, section, group_key,
                    item_name, code, description, qty, unit_price, unit_cost, line_discount, net_amount, cost_amount,
                    free, serial_no, warranty_until, ref_item_id)
                 VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, ?13, ?14, ?15, ?16, '', ?17)",
                params![
                    ret_id,
                    l.item.item_type,
                    l.item.product_id,
                    service_id,
                    section,
                    group_key,
                    item_name,
                    l.item.code,
                    l.item.description,
                    -l.qty,
                    l.item.unit_price,
                    unit_cost,
                    -l.net,
                    -l.cost,
                    l.item.free,
                    l.item.serial_no,
                    l.item.id
                ],
            )?;
            if let Some(pid) = l.item.product_id {
                self.move_stock_at(pid, l.qty, "return", &ret_no, unit_cost, note.trim(), &ts)?;
            }
        }
        let mut paid = 0;
        if let Some(method) = refund {
            let p = PaymentInput {
                method,
                amount: -refund_total,
                bank_name: String::new(),
                reference: orig.sale.invoice_no.clone(),
                note: "Refund".into(),
            };
            self.insert_payment(Some(ret_id), None, orig.sale.customer_id, &p, &ts)?;
            paid = -refund_total;
        }
        tx.commit()?;
        self.audit("return", &format!("{ret_no} for {} ({})", orig.sale.invoice_no, self.money().fmt(refund_total)));
        Ok(SaleReceipt { sale_id: ret_id, invoice_no: ret_no, total: -refund_total, paid, balance: -refund_total - paid })
    }

    /// Payments received on a given day (for the dashboard and cash closing).
    pub(crate) fn payments_between(&self, from_ts: &str, to_ts: &str, method: PayMethod) -> Result<Money> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM payments WHERE created_at >= ?1 AND created_at < ?2 AND method = ?3",
            params![from_ts, to_ts, method],
            |r| r.get(0),
        )?)
    }
}
