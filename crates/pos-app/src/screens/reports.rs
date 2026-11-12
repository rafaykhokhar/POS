//! Reports: profit sheet, sales list with returns, stock value, Excel export.

use crate::conv::{model, sale_row};
use crate::ctx::Ctx;
use crate::{DayBar, ProfitLine, ProfitSec, ReceiptLine, ReportsData, KV};
use chrono::{Datelike, Duration, Local, NaiveDate};
use pos_core::money::{fmt_percent, margin_bp, round_div};
use pos_core::util::{pretty_date, pretty_ts, to_csv};
use pos_core::{ItemType, LevelFilter, PayMethod, ProductFilter, SaleDetail, SaleKind, SalesFilter, Section};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct State {
    from: String,
    to: String,
    sale: Option<SaleDetail>,
    return_qty: Vec<i64>,
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State::default());
}

fn d(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// (from, to) for a preset range.
fn range_dates(i: i32) -> (String, String) {
    let t = Local::now().date_naive();
    match i {
        0 => (d(t), d(t)),
        1 => {
            let y = t - Duration::days(1);
            (d(y), d(y))
        }
        2 => (d(t - Duration::days(t.weekday().num_days_from_monday() as i64)), d(t)),
        3 => (d(t.with_day(1).unwrap_or(t)), d(t)),
        4 => {
            let first_this = t.with_day(1).unwrap_or(t);
            let last_prev = first_this - Duration::days(1);
            (d(last_prev.with_day(1).unwrap_or(last_prev)), d(last_prev))
        }
        5 => (d(NaiveDate::from_ymd_opt(t.year(), 1, 1).unwrap_or(t)), d(t)),
        _ => (d(t - Duration::days(30)), d(t)),
    }
}

fn section_color(s: Section) -> i32 {
    match s {
        Section::Vehicle => 1,
        Section::Part => 0,
        Section::Labor => 2,
        Section::WorkshopParts => 3,
        Section::Other => 4,
    }
}

fn set_dates(ctx: &Ctx, from: String, to: String) {
    let ui = ctx.ui();
    let r = ui.global::<ReportsData>();
    r.set_from(from.clone().into());
    r.set_to(to.clone().into());
    r.set_range_label(
        if from == to { pretty_date(&from) } else { format!("{} – {}", pretty_date(&from), pretty_date(&to)) }.into(),
    );
    ST.with(|st| {
        let mut st = st.borrow_mut();
        st.from = from;
        st.to = to;
    });
}

fn dates() -> (String, String) {
    ST.with(|st| {
        let st = st.borrow();
        (st.from.clone(), st.to.clone())
    })
}

pub fn refresh(ctx: &Rc<Ctx>) {
    if dates().0.is_empty() {
        let (f, t) = range_dates(3);
        set_dates(ctx, f, t);
    }
    let ui = ctx.ui();
    let r = ui.global::<ReportsData>();
    let (from, to) = dates();
    match r.get_tab() {
        0 => {
            if !ctx.pos.can(pos_core::Perm::ViewCost) {
                return;
            }
            let sheet = match ctx.pos.profit_sheet(&from, &to) {
                Ok(s) => s,
                Err(e) => {
                    ctx.err(e);
                    return;
                }
            };
            r.set_revenue(ctx.money(sheet.revenue));
            r.set_cost(ctx.money(sheet.cost));
            r.set_profit(ctx.money(sheet.profit.abs()));
            r.set_loss(sheet.profit < 0);
            r.set_margin(fmt_percent(margin_bp(sheet.profit, sheet.revenue)).into());
            let kv = |label: &str, value: String, tone: i32| KV { label: label.into(), value: value.into(), tone };
            r.set_extras(model(vec![
                kv("Bills", sheet.sales_count.to_string(), 0),
                kv("Discounts given", ctx.money(sheet.discounts).to_string(), 0),
                kv("Tax collected", ctx.money(sheet.tax).to_string(), 0),
                kv("Returns refunded", ctx.money(sheet.returns).to_string(), if sheet.returns > 0 { 1 } else { 0 }),
                kv("Warranty / free work cost", ctx.money(sheet.warranty_cost).to_string(), if sheet.warranty_cost > 0 { 1 } else { 0 }),
                kv("Net profit", ctx.money(sheet.profit).to_string(), 9),
            ]));
            let max_profit = sheet.sections.iter().map(|s| s.profit).max().unwrap_or(0).max(1);
            r.set_sections(model(
                sheet
                    .sections
                    .iter()
                    .map(|s| ProfitSec {
                        name: s.section.label().into(),
                        revenue: ctx.money(s.revenue),
                        cost: ctx.money(s.cost),
                        profit: ctx.money(s.profit),
                        margin: if s.revenue != 0 { fmt_percent(margin_bp(s.profit, s.revenue)).into() } else { "—".into() },
                        ratio: (s.profit.max(0) as f64 / max_profit as f64) as f32,
                        color: section_color(s.section),
                        negative: s.profit < 0,
                    })
                    .collect(),
            ));
            let max_rev = sheet.by_day.iter().map(|d| d.total.max(0)).max().unwrap_or(0).max(1);
            r.set_days(model(
                sheet
                    .by_day
                    .iter()
                    .map(|d| DayBar {
                        label: d.label.clone().into(),
                        value: ctx.money(d.total),
                        revenue: (d.total.max(0) as f64 / max_rev as f64) as f32,
                        profit: (d.profit.max(0) as f64 / max_rev as f64) as f32,
                    })
                    .collect(),
            ));
            let filter = r.get_item_filter();
            r.set_items(model(
                sheet
                    .items
                    .iter()
                    .filter(|i| match filter {
                        1 => i.section == Section::Vehicle,
                        2 => i.section == Section::Part,
                        3 => matches!(i.section, Section::Labor | Section::WorkshopParts),
                        _ => true,
                    })
                    .map(|i| ProfitLine {
                        section: i.section.label().into(),
                        color: section_color(i.section),
                        code: i.code.clone().into(),
                        name: i.name.clone().into(),
                        qty: i.qty.to_string().into(),
                        revenue: ctx.money(i.revenue),
                        cost: ctx.money(i.cost),
                        profit: ctx.money(i.profit),
                        margin: if i.revenue != 0 { fmt_percent(margin_bp(i.profit, i.revenue)).into() } else { "—".into() },
                        negative: i.profit < 0,
                    })
                    .collect(),
            ));
        }
        1 => {
            let kind = match r.get_sales_kind() {
                1 => Some(SaleKind::Counter),
                2 => Some(SaleKind::Vehicle),
                3 => Some(SaleKind::Workshop),
                4 => Some(SaleKind::Return),
                _ => None,
            };
            let list = ctx
                .pos
                .list_sales(&SalesFilter {
                    from: from.clone(),
                    to: to.clone(),
                    kind,
                    search: r.get_sales_search().to_string(),
                    limit: Some(2000),
                    ..Default::default()
                })
                .unwrap_or_default();
            r.set_sales(model(list.iter().map(|s| sale_row(ctx, s)).collect()));
            let total: i64 = list.iter().map(|s| s.total).sum();
            let bills = list.iter().filter(|s| s.kind != SaleKind::Return).count();
            let returns: i64 = list.iter().filter(|s| s.kind == SaleKind::Return).map(|s| -s.total).sum();
            let mut stats = vec![
                KV { label: "Total".into(), value: ctx.money(total), tone: 0 },
                KV { label: "Bills".into(), value: bills.to_string().into(), tone: 0 },
            ];
            if let Ok(sum) = ctx.pos.sales_summary(&from, &to) {
                for (m, amt) in sum.by_method {
                    stats.push(KV {
                        label: match m {
                            PayMethod::Cash => "Cash received".into(),
                            PayMethod::Bank => "Bank received".into(),
                        },
                        value: ctx.money(amt),
                        tone: 0,
                    });
                }
            }
            stats.push(KV { label: "Returns".into(), value: ctx.money(returns), tone: if returns > 0 { 1 } else { 0 } });
            r.set_sales_stats(model(stats));
        }
        _ => {
            let Ok(s) = ctx.pos.stock_report() else { return };
            let view_cost = ctx.pos.can(pos_core::Perm::ViewCost);
            let mut v = vec![
                KV { label: "Products".into(), value: s.products.to_string().into(), tone: 0 },
                KV { label: "Units in stock".into(), value: s.units.to_string().into(), tone: 0 },
            ];
            if view_cost {
                v.push(KV { label: "Parts value at cost".into(), value: ctx.money(s.value_cost), tone: 0 });
            }
            v.push(KV { label: "Parts value at sale price".into(), value: ctx.money(s.value_retail), tone: 0 });
            if view_cost {
                v.push(KV { label: "Profit if all parts sell".into(), value: ctx.money(s.value_retail - s.value_cost), tone: 2 });
            }
            v.push(KV { label: "Low stock items".into(), value: s.low.to_string().into(), tone: if s.low > 0 { 1 } else { 0 } });
            v.push(KV { label: "Out of stock items".into(), value: s.out.to_string().into(), tone: if s.out > 0 { 1 } else { 0 } });
            v.push(KV { label: "Bikes in stock (incl. booked)".into(), value: s.vehicles.to_string().into(), tone: 0 });
            if view_cost {
                v.push(KV { label: "Bikes value at cost".into(), value: ctx.money(s.vehicles_value), tone: 0 });
                v.push(KV { label: "Total stock value".into(), value: ctx.money(s.value_cost + s.vehicles_value), tone: 9 });
            }
            r.set_stock_stats(model(v));
        }
    }
}

fn save_csv(ctx: &Ctx, default_name: &str, content: String) {
    let dir = dirs::document_dir().or_else(dirs::home_dir).unwrap_or_default();
    let path = rfd::FileDialog::new()
        .set_title("Save report")
        .set_directory(&dir)
        .set_file_name(default_name)
        .add_filter("Excel / CSV", &["csv"])
        .save_file();
    let Some(path) = path else { return };
    // UTF-8 BOM so Excel shows symbols like "Rs" and "—" correctly.
    match std::fs::write(&path, format!("\u{feff}{content}")) {
        Ok(()) => ctx.ok(format!("Saved {}", path.file_name().and_then(|n| n.to_str()).unwrap_or("report"))),
        Err(e) => ctx.err(format!("Could not save: {e}")),
    }
}

fn plain(ctx: &Ctx, m: i64) -> String {
    ctx.pos.money().plain(m).replace(',', "")
}

fn open_sale(ctx: &Rc<Ctx>, id: i64) {
    let sale = match ctx.pos.get_sale(id) {
        Ok(s) => s,
        Err(e) => {
            ctx.err(e);
            return;
        }
    };
    let ui = ctx.ui();
    let r = ui.global::<ReportsData>();
    let is_return = sale.sale.kind == SaleKind::Return;
    r.set_sale_title(
        format!("{} {}", if is_return { "Return" } else { sale.sale.kind.label() }, sale.sale.invoice_no).into(),
    );
    let mut sub = vec![pretty_ts(&sale.sale.created_at)];
    if !sale.sale.customer_name.is_empty() {
        sub.push(format!(
            "{}{}",
            sale.sale.customer_name,
            if sale.customer_phone.is_empty() { String::new() } else { format!(" ({})", sale.customer_phone) }
        ));
    }
    if !sale.salesperson_name.is_empty() {
        sub.push(format!("by {}", sale.salesperson_name));
    }
    if !sale.original_invoice.is_empty() {
        sub.push(format!("for {}", sale.original_invoice));
    }
    r.set_sale_sub(sub.join(" · ").into());
    let lines: Vec<ReceiptLine> = sale
        .items
        .iter()
        .map(|i| {
            let mut notes = Vec::new();
            if !i.serial_no.is_empty() && i.item_type != ItemType::Vehicle {
                notes.push(format!("S/N {}", i.serial_no));
            }
            if !i.warranty_until.is_empty() {
                notes.push(format!("warranty until {}", pretty_date(&i.warranty_until)));
            }
            if i.line_discount > 0 {
                notes.push(format!("discount {}", ctx.money(i.line_discount)));
            }
            if i.free {
                notes.push("free (warranty / coupon)".into());
            }
            if i.returned_qty > 0 {
                notes.push(format!("{} returned", i.returned_qty));
            }
            let can = if is_return || i.item_type == ItemType::Vehicle { 0 } else { i.qty - i.returned_qty };
            ReceiptLine {
                item_id: i.id as i32,
                code: i.code.clone().into(),
                desc: i.description.clone().into(),
                qty: i.qty.abs().to_string().into(),
                price: ctx.money(i.unit_price),
                total: if i.free { "FREE".into() } else { ctx.money(i.unit_price * i.qty - i.line_discount) },
                note: notes.join(" · ").into(),
                can_return: can.max(0) as i32,
            }
        })
        .collect();
    let can_return = lines.iter().any(|l| l.can_return > 0);
    r.set_sale_lines(model(lines));
    let kv = |label: &str, value: String, tone: i32| KV { label: label.into(), value: value.into(), tone };
    let mut totals = vec![kv("Subtotal", ctx.money(sale.subtotal).to_string(), 0)];
    if sale.discount != 0 {
        totals.push(kv("Discount", format!("− {}", ctx.money(sale.discount)), 2));
    }
    if sale.tax != 0 {
        totals.push(kv("Tax", ctx.money(sale.tax).to_string(), 0));
    }
    totals.push(kv("Total", ctx.money(sale.sale.total).to_string(), 9));
    totals.push(kv("Paid", ctx.money(sale.sale.paid).to_string(), 0));
    let bal = sale.sale.balance();
    if bal != 0 {
        totals.push(kv(if bal > 0 { "Balance due" } else { "Credit" }, ctx.money(bal.abs()).to_string(), 1));
    }
    if ctx.pos.can(pos_core::Perm::ViewCost) {
        totals.push(kv("Profit on this bill", ctx.money(sale.sale.profit).to_string(), 2));
    }
    r.set_sale_totals(model(totals));
    r.set_sale_payments(model(
        sale.payments
            .iter()
            .map(|p| KV {
                label: format!(
                    "{}{} · {}",
                    p.method.label(),
                    if p.bank_name.is_empty() { String::new() } else { format!(" ({})", p.bank_name) },
                    pretty_ts(&p.created_at)
                )
                .into(),
                value: ctx.money(p.amount),
                tone: if p.amount < 0 { 1 } else { 0 },
            })
            .collect(),
    ));
    r.set_sale_can_return(can_return && sale.sale.kind != SaleKind::Vehicle);
    r.set_return_mode(false);
    r.set_return_error("".into());
    r.set_return_total(ctx.money(0));
    ST.with(|st| {
        let mut st = st.borrow_mut();
        st.return_qty = vec![0; sale.items.len()];
        st.sale = Some(sale);
    });
    r.set_show_sale(true);
}

fn return_preview(ctx: &Ctx) -> i64 {
    ST.with(|st| {
        let st = st.borrow();
        let Some(sale) = &st.sale else { return 0 };
        let orig_net: i64 = sale.items.iter().map(|i| i.net_amount).sum();
        let mut net = 0i64;
        for (i, q) in sale.items.iter().zip(st.return_qty.iter()) {
            if *q > 0 && i.qty > 0 {
                net += round_div(i.net_amount as i128 * *q as i128, i.qty as i128);
            }
        }
        let _ = ctx;
        if orig_net > 0 {
            round_div(net as i128 * sale.sale.total as i128, orig_net as i128)
        } else {
            net
        }
    })
}

/// Profit sheet as CSV: (suggested file name, contents).
pub(crate) fn profit_csv(ctx: &Ctx) -> Result<(String, String), String> {
    let (from, to) = dates();
    let sheet = ctx.pos.profit_sheet(&from, &to).map_err(|_| "Only managers and owners can export profit.".to_string())?;
    let mut rows: Vec<Vec<String>> = sheet
        .items
        .iter()
        .map(|i| {
            vec![
                i.section.label().to_string(),
                i.code.clone(),
                i.name.clone(),
                i.qty.to_string(),
                plain(ctx, i.revenue),
                plain(ctx, i.cost),
                plain(ctx, i.profit),
                fmt_percent(margin_bp(i.profit, i.revenue)),
            ]
        })
        .collect();
    rows.push(vec![]);
    for s in &sheet.sections {
        rows.push(vec![
            format!("TOTAL {}", s.section.label()),
            String::new(),
            String::new(),
            s.qty.to_string(),
            plain(ctx, s.revenue),
            plain(ctx, s.cost),
            plain(ctx, s.profit),
            fmt_percent(margin_bp(s.profit, s.revenue)),
        ]);
    }
    rows.push(vec![
        "GRAND TOTAL".into(),
        String::new(),
        String::new(),
        String::new(),
        plain(ctx, sheet.revenue),
        plain(ctx, sheet.cost),
        plain(ctx, sheet.profit),
        fmt_percent(margin_bp(sheet.profit, sheet.revenue)),
    ]);
    let csv = to_csv(&["Section", "Code", "Item", "Qty", "Sales", "Cost", "Profit", "Margin"], &rows);
    Ok((format!("profit-sheet-{from}-to-{to}.csv"), csv))
}

/// Sales list as CSV.
pub(crate) fn sales_csv(ctx: &Ctx) -> Result<(String, String), String> {
    let (from, to) = dates();
    let list = ctx
        .pos
        .list_sales(&SalesFilter { from: from.clone(), to: to.clone(), limit: Some(100_000), ..Default::default() })
        .unwrap_or_default();
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|s| {
            vec![
                s.invoice_no.clone(),
                s.created_at.clone(),
                s.kind.label().to_string(),
                s.customer_name.clone(),
                s.user_name.clone(),
                s.items.to_string(),
                plain(ctx, s.total),
                plain(ctx, s.paid),
                plain(ctx, s.balance()),
            ]
        })
        .collect();
    let csv = to_csv(&["Invoice", "Date", "Type", "Customer", "By", "Items", "Total", "Paid", "Balance"], &rows);
    Ok((format!("sales-{from}-to-{to}.csv"), csv))
}

/// Stock list as CSV.
pub(crate) fn stock_csv(ctx: &Ctx) -> Result<(String, String), String> {
    let list = ctx
        .pos
        .list_products(&ProductFilter { level: LevelFilter::All, ..Default::default() })
        .unwrap_or_default();
    let cost = ctx.pos.can(pos_core::Perm::ViewCost);
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|p| {
            let mut r = vec![
                p.code.clone(),
                p.name.clone(),
                p.category_name.clone(),
                p.stock.to_string(),
                p.low_stock.to_string(),
                match p.level() {
                    pos_core::StockLevel::Out => "OUT".into(),
                    pos_core::StockLevel::Low => "LOW".into(),
                    _ => String::new(),
                },
                plain(ctx, p.price),
            ];
            if cost {
                r.push(plain(ctx, p.cost));
                r.push(plain(ctx, p.cost * p.stock.max(0)));
            }
            r.push(plain(ctx, p.price * p.stock.max(0)));
            r
        })
        .collect();
    let mut headers = vec!["Code", "Product", "Category", "Stock", "Red at", "Status", "Price"];
    if cost {
        headers.extend(["Cost", "Value at cost"]);
    }
    headers.push("Value at price");
    let csv = to_csv(&headers, &rows);
    Ok((format!("stock-{}.csv", pos_core::util::today()), csv))
}

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let r = ui.global::<ReportsData>();
    {
        let ctx = ctx.clone();
        r.on_refresh(move || refresh(&ctx));
    }
    {
        let ctx = ctx.clone();
        r.on_set_range(move |i| {
            if i == 6 {
                return;
            }
            let (f, t) = range_dates(i);
            set_dates(&ctx, f, t);
            refresh(&ctx);
        });
    }
    {
        let ctx = ctx.clone();
        r.on_apply_custom(move |f, t| {
            let (Some(a), Some(b)) = (pos_core::util::parse_date(&f), pos_core::util::parse_date(&t)) else {
                ctx.err("Dates must look like 2026-11-01");
                return;
            };
            let (a, b) = if a <= b { (a, b) } else { (b, a) };
            set_dates(&ctx, d(a), d(b));
            refresh(&ctx);
        });
    }
    {
        let ctx = ctx.clone();
        r.on_export_profit(move || match profit_csv(&ctx) {
            Ok((name, csv)) => save_csv(&ctx, &name, csv),
            Err(e) => ctx.err(e),
        });
    }
    {
        let ctx = ctx.clone();
        r.on_export_sales(move || match sales_csv(&ctx) {
            Ok((name, csv)) => save_csv(&ctx, &name, csv),
            Err(e) => ctx.err(e),
        });
    }
    {
        let ctx = ctx.clone();
        r.on_export_stock(move || match stock_csv(&ctx) {
            Ok((name, csv)) => save_csv(&ctx, &name, csv),
            Err(e) => ctx.err(e),
        });
    }
    {
        let ctx = ctx.clone();
        r.on_open_sale(move |id| open_sale(&ctx, id as i64));
    }
    {
        let ctx = ctx.clone();
        r.on_print_sale(move |paper| {
            let id = ST.with(|st| st.borrow().sale.as_ref().map(|s| s.sale.id));
            if let Some(id) = id {
                ctx.print_sale(id, if paper == 1 { crate::print::Paper::A4 } else { crate::print::Paper::Thermal });
            }
        });
    }
    {
        let ctx = ctx.clone();
        r.on_set_return_qty(move |i, t| {
            let q = t.trim().parse::<i64>().unwrap_or(0).max(0);
            ST.with(|st| {
                let mut st = st.borrow_mut();
                if let Some(slot) = st.return_qty.get_mut(i as usize) {
                    *slot = q;
                }
            });
            let total = return_preview(&ctx);
            ctx.ui().global::<ReportsData>().set_return_total(ctx.money(total));
        });
    }
    {
        let ctx = ctx.clone();
        r.on_do_return(move |refund, note| {
            let ui = ctx.ui();
            let r = ui.global::<ReportsData>();
            let (sale_id, lines) = ST.with(|st| {
                let st = st.borrow();
                let Some(sale) = &st.sale else { return (0, vec![]) };
                let lines: Vec<(i64, i64)> = sale
                    .items
                    .iter()
                    .zip(st.return_qty.iter())
                    .filter(|(_, q)| **q > 0)
                    .map(|(i, q)| (i.id, *q))
                    .collect();
                (sale.sale.id, lines)
            });
            let method = match refund {
                0 => Some(PayMethod::Cash),
                1 => Some(PayMethod::Bank),
                _ => None,
            };
            match ctx.pos.return_items(sale_id, &lines, method, &note) {
                Ok(rec) => {
                    ctx.ok(format!("Return {} saved · refund {}", rec.invoice_no, ctx.money(-rec.total)));
                    open_sale(&ctx, sale_id);
                    refresh(&ctx);
                    ctx.update_badges();
                    true
                }
                Err(e) => {
                    r.set_return_error(e.to_string().into());
                    false
                }
            }
        });
    }
}

/// Open a sale's detail from elsewhere (dashboard).
pub fn show_sale(ctx: &Rc<Ctx>, id: i64) {
    let ui = ctx.ui();
    ui.global::<ReportsData>().set_tab(1);
    refresh(ctx);
    open_sale(ctx, id);
}
