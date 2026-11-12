//! Customers: list, profile, balance payments.

use crate::conv::{initials, model, money_in};
use crate::ctx::Ctx;
use crate::{CustForm, CustRow, CustomersData, LineRow};
use pos_core::util::{pretty_date, pretty_ts, today};
use pos_core::{Customer, CustomerInput, PaymentInput, SaleKind};
use slint::ComponentHandle;
use std::cell::Cell;
use std::rc::Rc;

thread_local! {
    static DETAIL: Cell<i64> = const { Cell::new(0) };
}

fn row(ctx: &Ctx, c: &Customer) -> CustRow {
    CustRow {
        id: c.id as i32,
        name: c.name.clone().into(),
        initials: initials(&c.name).into(),
        phone: c.phone.clone().into(),
        id_no: c.id_no.clone().into(),
        bikes: c.vehicles as i32,
        spent: ctx.money(c.total_spent),
        balance: ctx.money(c.balance),
        owing: c.balance > 0,
        since: pretty_date(&c.created_at).into(),
    }
}

pub fn refresh(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let d = ui.global::<CustomersData>();
    let list = match ctx.pos.list_customers(&d.get_search(), d.get_filter() == 1, 2000) {
        Ok(l) => l,
        Err(e) => {
            ctx.err(e);
            return;
        }
    };
    d.set_rows(model(list.iter().map(|c| row(ctx, c)).collect()));
    let all = ctx.pos.list_customers("", true, 100_000).unwrap_or_default();
    let owed: i64 = all.iter().map(|c| c.balance).sum();
    let total = ctx.pos.list_customers("", false, 100_000).map(|v| v.len()).unwrap_or(0);
    d.set_summary(
        if owed > 0 {
            format!("{total} customers · {} owed to you by {} customers", ctx.money(owed), all.len())
        } else {
            format!("{total} customers · nobody owes you money")
        }
        .into(),
    );
}

fn open(ctx: &Rc<Ctx>, id: i64) {
    let det = match ctx.pos.customer_detail(id) {
        Ok(d) => d,
        Err(e) => {
            ctx.err(e);
            return;
        }
    };
    DETAIL.with(|c| c.set(id));
    let ui = ctx.ui();
    let d = ui.global::<CustomersData>();
    d.set_detail(row(ctx, &det.customer));
    d.set_detail_address(det.customer.address.clone().into());
    d.set_detail_email(det.customer.email.clone().into());
    d.set_detail_notes(det.customer.notes.clone().into());
    let t = today();
    d.set_bikes(model(
        det.vehicles
            .iter()
            .map(|v| {
                let warranty = !v.vehicle_warranty_until.is_empty() && v.vehicle_warranty_until.as_str() >= t.as_str();
                LineRow {
                    title: format!("{}{}", v.label(), if v.color.is_empty() { String::new() } else { format!(" · {}", v.color) }).into(),
                    sub: if v.sold_at.is_empty() {
                        "Serviced here (bought elsewhere)".to_string()
                    } else {
                        format!(
                            "Bought {} · {} free services left",
                            pretty_date(&v.sold_at),
                            v.coupons_left
                        )
                    }
                    .into(),
                    right: if v.sold_at.is_empty() { "".into() } else { ctx.money(v.price) },
                    badge: if v.sold_at.is_empty() {
                        "".into()
                    } else if warranty {
                        "Under warranty".into()
                    } else {
                        "Warranty ended".into()
                    },
                    tone: if warranty { 2 } else { 0 },
                }
            })
            .collect(),
    ));
    d.set_sales(model(
        det.sales
            .iter()
            .map(|s| LineRow {
                title: format!("{} · {} items", s.invoice_no, s.items.abs()).into(),
                sub: format!(
                    "{}{}",
                    pretty_ts(&s.created_at),
                    if s.balance() > 0 { format!(" · {} still due", ctx.money(s.balance())) } else { String::new() }
                )
                .into(),
                right: ctx.money(s.total),
                badge: s.kind.label().into(),
                tone: match s.kind {
                    SaleKind::Return => 1,
                    SaleKind::Vehicle => 3,
                    SaleKind::Workshop => 4,
                    SaleKind::Counter => 2,
                },
            })
            .collect(),
    ));
    d.set_jobs(model(
        det.jobs
            .iter()
            .map(|j| LineRow {
                title: format!("{} · {}", j.job_no, j.vehicle_label).into(),
                sub: format!("{} · {}", pretty_date(&j.created_at), j.complaint).into(),
                right: ctx.money(j.total()),
                badge: j.status.label().into(),
                tone: if j.status.is_open() { 5 } else { 0 },
            })
            .collect(),
    ));
    d.set_payments(model(
        det.payments
            .iter()
            .map(|p| LineRow {
                title: format!("{} · {}", p.method.label(), if p.reference_no.is_empty() { "—" } else { &p.reference_no }).into(),
                sub: format!(
                    "{}{}{}",
                    pretty_ts(&p.created_at),
                    if p.bank_name.is_empty() { String::new() } else { format!(" · {}", p.bank_name) },
                    if p.note.is_empty() { String::new() } else { format!(" · {}", p.note) }
                )
                .into(),
                right: ctx.money(p.amount),
                badge: if p.amount < 0 { "Refund".into() } else { "".into() },
                tone: 1,
            })
            .collect(),
    ));
    d.set_show_detail(true);
}

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let d = ui.global::<CustomersData>();
    {
        let ctx = ctx.clone();
        d.on_refresh(move || refresh(&ctx));
    }
    {
        let ctx = ctx.clone();
        d.on_new_customer(move || {
            let ui = ctx.ui();
            let d = ui.global::<CustomersData>();
            d.set_form(CustForm::default());
            d.set_form_error("".into());
            d.set_show_form(true);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_edit(move |id| {
            if let Ok(c) = ctx.pos.get_customer(id as i64) {
                let ui = ctx.ui();
                let d = ui.global::<CustomersData>();
                d.set_form(CustForm {
                    id: c.id as i32,
                    name: c.name.into(),
                    phone: c.phone.into(),
                    id_no: c.id_no.into(),
                    address: c.address.into(),
                    email: c.email.into(),
                    notes: c.notes.into(),
                });
                d.set_form_error("".into());
                d.set_show_form(true);
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_save(move |f| {
            let ui = ctx.ui();
            let d = ui.global::<CustomersData>();
            let r = ctx.pos.save_customer(CustomerInput {
                id: if f.id > 0 { Some(f.id as i64) } else { None },
                name: f.name.to_string(),
                phone: f.phone.to_string(),
                id_no: f.id_no.to_string(),
                address: f.address.to_string(),
                email: f.email.to_string(),
                notes: f.notes.to_string(),
            });
            match r {
                Ok(id) => {
                    d.set_show_form(false);
                    ctx.ok("Customer saved");
                    refresh(&ctx);
                    if d.get_show_detail() {
                        open(&ctx, id);
                    }
                    true
                }
                Err(e) => {
                    d.set_form_error(e.to_string().into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_open(move |id| {
            ctx.ui().global::<CustomersData>().set_detail_tab(0);
            open(&ctx, id as i64);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_receive(move |method, amount, reference| {
            let ui = ctx.ui();
            let d = ui.global::<CustomersData>();
            let id = DETAIL.with(|c| c.get());
            let amt = match money_in(&amount, "Amount") {
                Ok(a) => a,
                Err(e) => {
                    d.set_pay_error(e.into());
                    return false;
                }
            };
            let p = if method == 1 { PaymentInput::bank(amt, &reference, &reference) } else { PaymentInput::cash(amt) };
            match ctx.pos.receive_customer_payment(id, p) {
                Ok(()) => {
                    d.set_show_pay(false);
                    ctx.ok(format!("Payment of {} received", ctx.money(amt)));
                    refresh(&ctx);
                    open(&ctx, id);
                    ctx.update_badges();
                    true
                }
                Err(e) => {
                    d.set_pay_error(e.to_string().into());
                    false
                }
            }
        });
    }
}
