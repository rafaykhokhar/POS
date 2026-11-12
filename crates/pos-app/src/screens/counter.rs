//! Counter sales: code entry, cart, discounts, hold/resume and payment.

use crate::conv::{model, product_hit};
use crate::ctx::Ctx;
use crate::{CartRow, CounterData, HeldRow, PayForm};
use pos_core::money::parse_money;
use pos_core::{CartLine, CheckoutInput, ItemType, Money, PaymentInput, Product, SaleKind};
use slint::{ComponentHandle, SharedString};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone)]
struct Line {
    product_id: i64,
    code: String,
    name: String,
    unit_price: Money,
    qty: i64,
    discount: Money,
    stock: i64,
    low: i64,
    unit: String,
}

#[derive(Clone)]
struct Held {
    lines: Vec<Line>,
    customer: (i32, SharedString, SharedString),
    discount: Money,
    time: String,
}

#[derive(Default)]
struct State {
    lines: Vec<Line>,
    discount: Money,
    held: Vec<Held>,
    last_sale: i64,
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State::default());
}

fn cart_lines(lines: &[Line]) -> Vec<CartLine> {
    lines
        .iter()
        .map(|l| CartLine {
            item_type: ItemType::Product,
            ref_id: Some(l.product_id),
            code: l.code.clone(),
            description: l.name.clone(),
            qty: l.qty,
            unit_price: l.unit_price,
            line_discount: l.discount,
            serial_no: String::new(),
            free: false,
        })
        .collect()
}

/// "2*BRK-001", "BRK-001*2" -> (2, "BRK-001")
fn parse_entry(q: &str) -> (i64, String) {
    let q = q.trim();
    if let Some((a, b)) = q.split_once('*') {
        let (a, b) = (a.trim(), b.trim());
        if let Ok(n) = a.parse::<i64>() {
            return (n.max(1), b.to_string());
        }
        if let Ok(n) = b.parse::<i64>() {
            return (n.max(1), a.to_string());
        }
    }
    (1, q.to_string())
}

fn total_now(ctx: &Ctx) -> Money {
    ST.with(|st| {
        let st = st.borrow();
        ctx.pos.totals(&cart_lines(&st.lines), st.discount).total
    })
}

pub fn render(ctx: &Ctx) {
    let ui = ctx.ui();
    let c = ui.global::<CounterData>();
    let settings = ctx.pos.settings();
    ST.with(|st| {
        let st = st.borrow();
        let lines = cart_lines(&st.lines);
        let t = ctx.pos.totals(&lines, st.discount);
        let rows: Vec<CartRow> = st
            .lines
            .iter()
            .zip(lines.iter())
            .map(|(l, cl)| {
                let left = l.stock - l.qty;
                let level = pos_core::stock_level(l.stock, l.low);
                CartRow {
                    code: l.code.clone().into(),
                    name: l.name.clone().into(),
                    price: ctx.money(l.unit_price),
                    qty: l.qty as i32,
                    discount: if l.discount > 0 { ctx.pos.money().edit(l.discount).into() } else { SharedString::new() },
                    has_discount: l.discount > 0,
                    total: ctx.money(cl.net()),
                    stock: l.stock as i32,
                    level: crate::conv::level_num(level),
                    info: if left <= 0 {
                        "Last ones in stock".into()
                    } else {
                        format!("{left} {} left after this sale", l.unit).into()
                    },
                }
            })
            .collect();
        c.set_cart(model(rows));
        c.set_items_count(t.items.to_string().into());
        c.set_subtotal(ctx.money(t.gross));
        let disc = t.discount + t.line_discounts;
        c.set_has_discount(disc > 0);
        c.set_discount_total(ctx.money(disc));
        c.set_show_tax(t.tax > 0);
        c.set_tax(ctx.money(t.tax));
        c.set_tax_label(
            format!(
                "{} {}{}",
                settings.tax_label,
                pos_core::money::fmt_percent(settings.tax_rate_bp),
                if settings.tax_inclusive { " (included)" } else { "" }
            )
            .into(),
        );
        c.set_total(ctx.money(t.total));
        c.set_total_plain(ctx.pos.money().edit(t.total).into());
        c.set_quick_amounts(crate::conv::strings(quick_amounts(t.total)));
        let held: Vec<HeldRow> = st
            .held
            .iter()
            .map(|h| {
                let t = ctx.pos.totals(&cart_lines(&h.lines), h.discount);
                HeldRow {
                    label: if h.customer.0 > 0 { h.customer.1.clone() } else { "Walk-in customer".into() },
                    items: format!("{} items", t.items).into(),
                    total: ctx.money(t.total),
                    time: h.time.clone().into(),
                }
            })
            .collect();
        c.set_held(model(held));
    });
}

/// Suggested cash amounts: exact, then round numbers above the total.
fn quick_amounts(total: Money) -> Vec<String> {
    let units = (total + 99) / 100;
    let mut out = vec![format!("{}", units)];
    for step in [100, 500, 1000, 5000] {
        let v = ((units + step - 1) / step) * step;
        let s = format!("{v}");
        if v > units && !out.contains(&s) {
            out.push(s);
        }
    }
    out.truncate(4);
    out
}

fn set_message(ctx: &Ctx, m: &str) {
    ctx.ui().global::<CounterData>().set_message(m.into());
}

fn add_product(ctx: &Ctx, p: &Product, qty: i64) {
    let allow_negative = ctx.pos.settings().allow_negative_stock;
    let ok = ST.with(|st| {
        let mut st = st.borrow_mut();
        let existing: i64 = st.lines.iter().filter(|l| l.product_id == p.id).map(|l| l.qty).sum();
        if existing + qty > p.stock && !allow_negative {
            let msg = if p.stock <= 0 {
                format!("{} ({}) is out of stock.", p.name, p.code)
            } else {
                format!("Only {} of {} ({}) in stock.", p.stock, p.name, p.code)
            };
            set_message(ctx, &msg);
            return false;
        }
        if let Some(l) = st.lines.iter_mut().find(|l| l.product_id == p.id && l.discount == 0) {
            l.qty += qty;
        } else {
            st.lines.push(Line {
                product_id: p.id,
                code: p.code.clone(),
                name: p.name.clone(),
                unit_price: p.price,
                qty,
                discount: 0,
                stock: p.stock,
                low: p.low_stock,
                unit: p.unit.clone(),
            });
        }
        true
    });
    if ok {
        let ui = ctx.ui();
        let c = ui.global::<CounterData>();
        c.set_query("".into());
        c.set_hits(model(vec![]));
        c.set_message("".into());
        render(ctx);
    }
}

fn reset(ctx: &Ctx) {
    ST.with(|st| {
        let mut st = st.borrow_mut();
        st.lines.clear();
        st.discount = 0;
    });
    let ui = ctx.ui();
    let c = ui.global::<CounterData>();
    c.set_customer_id(0);
    c.set_customer_name("".into());
    c.set_customer_phone("".into());
    c.set_customer_owing(false);
    c.set_discount_text("".into());
    c.set_query("".into());
    c.set_hits(model(vec![]));
    c.set_message("".into());
    render(ctx);
}

pub fn refresh(ctx: &Rc<Ctx>) {
    // Refresh stock numbers of items already in the cart.
    ST.with(|st| {
        let mut st = st.borrow_mut();
        for l in st.lines.iter_mut() {
            if let Ok(p) = ctx.pos.get_product(l.product_id) {
                l.stock = p.stock;
                l.low = p.low_stock;
            }
        }
    });
    render(ctx);
}

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let c = ui.global::<CounterData>();

    {
        let ctx = ctx.clone();
        c.on_query_changed(move |q| {
            let ui = ctx.ui();
            let c = ui.global::<CounterData>();
            c.set_message("".into());
            let (_, term) = parse_entry(&q);
            if term.is_empty() {
                c.set_hits(model(vec![]));
                return;
            }
            let hits = ctx.pos.search_products(&term, 8).unwrap_or_default();
            c.set_hits(model(hits.iter().map(|p| product_hit(&ctx, p)).collect()));
            c.set_hit_index(0);
        });
    }
    {
        let ctx = ctx.clone();
        c.on_submit_query(move |q| {
            let (qty, code) = parse_entry(&q);
            if code.is_empty() {
                return;
            }
            match ctx.pos.find_by_code(&code) {
                Ok(Some(p)) => add_product(&ctx, &p, qty),
                _ => {
                    let ui = ctx.ui();
                    let c = ui.global::<CounterData>();
                    let hits = ctx.pos.search_products(&code, 8).unwrap_or_default();
                    let idx = (c.get_hit_index().max(0) as usize).min(hits.len().saturating_sub(1));
                    if let Some(p) = hits.get(idx) {
                        add_product(&ctx, p, qty);
                    } else {
                        set_message(&ctx, &format!("No product with code or name “{code}”."));
                    }
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        c.on_pick(move |id| {
            let (qty, _) = parse_entry(&ctx.ui().global::<CounterData>().get_query());
            if let Ok(p) = ctx.pos.get_product(id as i64) {
                add_product(&ctx, &p, qty);
            }
        });
    }
    {
        let ctx = ctx.clone();
        c.on_set_qty(move |i, q| {
            let allow_negative = ctx.pos.settings().allow_negative_stock;
            let msg = ST.with(|st| {
                let mut st = st.borrow_mut();
                let Some(l) = st.lines.get_mut(i as usize) else { return None };
                let q = (q as i64).max(1);
                if q > l.stock && !allow_negative {
                    return Some(format!("Only {} of {} in stock.", l.stock, l.name));
                }
                l.qty = q;
                None
            });
            set_message(&ctx, msg.as_deref().unwrap_or(""));
            render(&ctx);
        });
    }
    {
        let ctx = ctx.clone();
        c.on_set_line_discount(move |i, t| {
            let Some(v) = parse_money(&t) else {
                set_message(&ctx, "Discount must be an amount, e.g. 200");
                return;
            };
            ST.with(|st| {
                let mut st = st.borrow_mut();
                if let Some(l) = st.lines.get_mut(i as usize) {
                    l.discount = v.clamp(0, l.unit_price * l.qty);
                }
            });
            set_message(&ctx, "");
            render(&ctx);
        });
    }
    {
        let ctx = ctx.clone();
        c.on_remove(move |i| {
            ST.with(|st| {
                let mut st = st.borrow_mut();
                if (i as usize) < st.lines.len() {
                    st.lines.remove(i as usize);
                }
            });
            render(&ctx);
        });
    }
    {
        let ctx = ctx.clone();
        c.on_clear(move || reset(&ctx));
    }
    {
        let ctx = ctx.clone();
        c.on_set_discount(move |t| {
            let v = parse_money(&t).unwrap_or(0).max(0);
            ST.with(|st| st.borrow_mut().discount = v);
            render(&ctx);
        });
    }
    {
        let ctx = ctx.clone();
        c.on_hold(move || {
            let ui = ctx.ui();
            let c = ui.global::<CounterData>();
            let held = ST.with(|st| {
                let mut st = st.borrow_mut();
                if st.lines.is_empty() {
                    return false;
                }
                let h = Held {
                    lines: std::mem::take(&mut st.lines),
                    customer: (c.get_customer_id(), c.get_customer_name(), c.get_customer_phone()),
                    discount: st.discount,
                    time: chrono::Local::now().format("%-I:%M %p").to_string(),
                };
                st.held.push(h);
                st.discount = 0;
                true
            });
            if held {
                reset(&ctx);
                ctx.ok("Bill put on hold");
            }
        });
    }
    {
        let ctx = ctx.clone();
        c.on_resume(move |i| {
            let ui = ctx.ui();
            let c = ui.global::<CounterData>();
            let current_customer = (c.get_customer_id(), c.get_customer_name(), c.get_customer_phone());
            let restored = ST.with(|st| {
                let mut st = st.borrow_mut();
                if (i as usize) >= st.held.len() {
                    return None;
                }
                let h = st.held.remove(i as usize);
                if !st.lines.is_empty() {
                    let cur = Held {
                        lines: std::mem::take(&mut st.lines),
                        customer: current_customer.clone(),
                        discount: st.discount,
                        time: chrono::Local::now().format("%-I:%M %p").to_string(),
                    };
                    st.held.push(cur);
                }
                st.lines = h.lines.clone();
                st.discount = h.discount;
                Some(h)
            });
            if let Some(h) = restored {
                c.set_customer_id(h.customer.0);
                c.set_customer_name(h.customer.1.clone());
                c.set_customer_phone(h.customer.2.clone());
                c.set_discount_text(if h.discount > 0 { ctx.pos.money().edit(h.discount).into() } else { "".into() });
                render(&ctx);
            }
        });
    }
    {
        let ctx = ctx.clone();
        c.on_discard_held(move |i| {
            ST.with(|st| {
                let mut st = st.borrow_mut();
                if (i as usize) < st.held.len() {
                    st.held.remove(i as usize);
                }
            });
            render(&ctx);
        });
    }
    {
        let ctx = ctx.clone();
        c.on_customer_changed(move |id| {
            if id > 0 {
                if let Ok(cu) = ctx.pos.get_customer(id as i64) {
                    let ui = ctx.ui();
                    let c = ui.global::<CounterData>();
                    c.set_customer_balance(ctx.money(cu.balance));
                    c.set_customer_owing(cu.balance > 0);
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        c.on_open_pay(move || {
            let empty = ST.with(|st| st.borrow().lines.is_empty());
            if empty {
                return;
            }
            let ui = ctx.ui();
            let c = ui.global::<CounterData>();
            c.set_pay_error("".into());
            c.set_show_pay(true);
        });
    }
    {
        let ctx = ctx.clone();
        c.on_pay_preview(move |f| -> SharedString {
            let total = total_now(&ctx);
            let has_customer = ctx.ui().global::<CounterData>().get_customer_id() > 0;
            let (cash, bank) = amounts(&f);
            let (Some(cash), Some(bank)) = (cash, bank) else { return "Enter a valid amount".into() };
            let received = cash + bank;
            if received >= total {
                let change = if f.method == 1 { 0 } else { received - total };
                if f.method == 1 && received > total {
                    return "Bank amount is more than the total".into();
                }
                if change > 0 {
                    format!("Change to give: {}", ctx.money(change)).into()
                } else {
                    "Paid in full ✓".into()
                }
            } else if has_customer {
                format!("Balance on account: {}", ctx.money(total - received)).into()
            } else {
                format!("Short by {}", ctx.money(total - received)).into()
            }
        });
    }
    {
        let ctx = ctx.clone();
        c.on_pay(move |f| pay(&ctx, &f));
    }
    {
        let ctx = ctx.clone();
        c.on_print_receipt(move |paper| {
            let id = ST.with(|st| st.borrow().last_sale);
            if id > 0 {
                ctx.print_sale(id, if paper == 1 { crate::print::Paper::A4 } else { crate::print::Paper::Thermal });
            }
        });
    }
    {
        let ctx = ctx.clone();
        c.on_new_sale(move || {
            let ui = ctx.ui();
            ui.global::<CounterData>().set_show_done(false);
            reset(&ctx);
        });
    }
    render(ctx);
}

fn amounts(f: &PayForm) -> (Option<Money>, Option<Money>) {
    match f.method {
        0 => (parse_money(&f.cash), Some(0)),
        1 => (Some(0), parse_money(&f.bank)),
        _ => (parse_money(&f.cash), parse_money(&f.bank)),
    }
}

fn pay(ctx: &Rc<Ctx>, f: &PayForm) -> bool {
    let ui = ctx.ui();
    let c = ui.global::<CounterData>();
    let (lines, discount) = ST.with(|st| {
        let st = st.borrow();
        (cart_lines(&st.lines), st.discount)
    });
    if lines.is_empty() {
        return false;
    }
    let total = ctx.pos.totals(&lines, discount).total;
    let (Some(cash), Some(bank)) = amounts(f) else {
        c.set_pay_error("Please enter valid amounts.".into());
        return false;
    };
    if cash < 0 || bank < 0 {
        c.set_pay_error("Amounts can't be negative.".into());
        return false;
    }
    if bank > total {
        c.set_pay_error("The bank amount is more than the total.".into());
        return false;
    }
    // Cash over the remaining amount is change, not income.
    let cash_kept = cash.min(total - bank);
    let change = cash - cash_kept;
    let mut payments = Vec::new();
    if cash_kept > 0 {
        payments.push(PaymentInput::cash(cash_kept));
    }
    if bank > 0 {
        payments.push(PaymentInput::bank(bank, &f.bank_name, &f.reference));
    }
    let customer = c.get_customer_id();
    let input = CheckoutInput {
        kind: SaleKind::Counter,
        customer_id: if customer > 0 { Some(customer as i64) } else { None },
        salesperson_id: None,
        items: lines,
        discount,
        payments,
        notes: String::new(),
        booking_id: None,
    };
    match ctx.pos.checkout(input) {
        Ok(r) => {
            c.set_done_invoice(r.invoice_no.clone().into());
            c.set_done_total(ctx.money(r.total));
            c.set_done_change(ctx.money(change));
            c.set_done_has_change(change > 0);
            c.set_done_balance(ctx.money(r.balance));
            c.set_done_has_balance(r.balance > 0);
            c.set_show_pay(false);
            c.set_show_done(true);
            ST.with(|st| {
                let mut st = st.borrow_mut();
                st.lines.clear();
                st.discount = 0;
                st.last_sale = r.sale_id;
            });
            render(ctx);
            ctx.update_badges();
            true
        }
        Err(e) => {
            c.set_pay_error(e.to_string().into());
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_parsing() {
        assert_eq!(parse_entry("2*BRK-001"), (2, "BRK-001".into()));
        assert_eq!(parse_entry("BRK-001*3"), (3, "BRK-001".into()));
        assert_eq!(parse_entry(" brk-001 "), (1, "brk-001".into()));
    }

    #[test]
    fn quick_cash() {
        assert_eq!(quick_amounts(1_250_000), vec!["12500", "13000", "15000"]);
        assert_eq!(quick_amounts(100_000), vec!["1000", "5000"]);
    }
}
