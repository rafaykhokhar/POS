//! Workshop: job board, job cards, parts & labour, delivery.

use crate::conv::{initials, int_in, model, money_in, strings};
use crate::ctx::Ctx;
use crate::{BikeHit, DeliverForm, JobColumn, JobDetail, JobEdit, JobItemRow, JobTile, NewJobForm, WorkshopData};
use chrono::NaiveDateTime;
use pos_core::util::{add_days, pretty_date, today, TS_FMT};
use pos_core::{JobCard, JobFilter, JobInput, JobItemKind, JobStatus, JobUpdate, LaborService, PaymentInput, User};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct State {
    techs: Vec<User>,
    services: Vec<LaborService>,
    job_id: i64,
    new_bike: i64,
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State::default());
}

fn age(ts: &str) -> String {
    let Ok(t) = NaiveDateTime::parse_from_str(ts, TS_FMT) else { return String::new() };
    let mins = (chrono::Local::now().naive_local() - t).num_minutes();
    if mins < 1 {
        "just now".into()
    } else if mins < 60 {
        format!("{mins} min ago")
    } else if mins < 60 * 24 {
        format!("{} h ago", mins / 60)
    } else if mins < 60 * 24 * 7 {
        format!("{} days ago", mins / 60 / 24)
    } else {
        pretty_date(ts)
    }
}

fn tile(ctx: &Ctx, j: &JobCard) -> JobTile {
    JobTile {
        id: j.id as i32,
        no: j.job_no.clone().into(),
        bike: j.vehicle_label.clone().into(),
        customer: if j.customer_name.is_empty() { "Walk-in".into() } else { j.customer_name.clone().into() },
        phone: j.customer_phone.clone().into(),
        complaint: j.complaint.clone().into(),
        tech: j.technician_name.clone().into(),
        tech_initials: initials(&j.technician_name).into(),
        tech_color: j.technician_id.map(|i| (i % 6) as i32).unwrap_or(0),
        age: if j.status.is_open() { age(&j.created_at) } else { pretty_date(&j.created_at) }.into(),
        total: ctx.money(j.total()),
        warranty: j.is_warranty,
        coupon: j.coupon_seq > 0,
        status: j.status.label().into(),
        status_code: j.status.index() as i32,
        invoice: j.invoice_no.clone().into(),
    }
}

fn load_lists(ctx: &Ctx) {
    let techs = ctx.pos.technicians().unwrap_or_default();
    let services = ctx.pos.list_services(false).unwrap_or_default();
    let ui = ctx.ui();
    let d = ui.global::<WorkshopData>();
    d.set_tech_names(strings(techs.iter().map(|t| format!("{} ({})", t.name, t.role.label())).collect()));
    d.set_service_names(strings(services.iter().map(|s| format!("{} · {}", s.name, ctx.pos.money().fmt(s.price))).collect()));
    d.set_service_prices(strings(services.iter().map(|s| ctx.pos.money().edit(s.price)).collect()));
    ST.with(|st| {
        let mut st = st.borrow_mut();
        st.techs = techs;
        st.services = services;
    });
}

pub fn refresh(ctx: &Rc<Ctx>) {
    load_lists(ctx);
    let ui = ctx.ui();
    let d = ui.global::<WorkshopData>();
    if d.get_view() == 0 {
        let jobs = match ctx.pos.list_jobs(&JobFilter { open_only: true, ..Default::default() }) {
            Ok(j) => j,
            Err(e) => {
                ctx.err(e);
                return;
            }
        };
        let cols: Vec<JobColumn> = JobStatus::BOARD
            .iter()
            .map(|s| {
                let mut list: Vec<&JobCard> = jobs.iter().filter(|j| j.status == *s).collect();
                list.sort_by_key(|j| j.id);
                JobColumn {
                    title: s.label().into(),
                    code: s.index() as i32,
                    jobs: model(list.iter().map(|j| tile(ctx, j)).collect()),
                }
            })
            .collect();
        d.set_columns(model(cols));
    } else {
        let jobs = ctx
            .pos
            .list_jobs(&JobFilter { search: d.get_search().to_string(), limit: Some(500), ..Default::default() })
            .unwrap_or_default();
        d.set_history(model(jobs.iter().map(|j| tile(ctx, j)).collect()));
    }
}

fn show_job(ctx: &Rc<Ctx>, id: i64) {
    let j = match ctx.pos.get_job(id) {
        Ok(j) => j,
        Err(e) => {
            ctx.err(e);
            return;
        }
    };
    ST.with(|st| st.borrow_mut().job_id = j.id);
    let tech_index = ST.with(|st| {
        j.technician_id.and_then(|t| st.borrow().techs.iter().position(|u| u.id == t)).map(|i| i as i32).unwrap_or(-1)
    });
    let ui = ctx.ui();
    let d = ui.global::<WorkshopData>();
    let next = if j.next_service_date.is_empty() {
        let days = ctx.pos.settings().next_service_days;
        if days > 0 { add_days(&today(), days) } else { String::new() }
    } else {
        j.next_service_date.clone()
    };
    d.set_job(JobDetail {
        id: j.id as i32,
        no: j.job_no.clone().into(),
        bike: j.vehicle_label.clone().into(),
        chassis: j.chassis_no.clone().into(),
        customer: if j.customer_name.is_empty() { "Walk-in".into() } else { j.customer_name.clone().into() },
        phone: j.customer_phone.clone().into(),
        status: j.status.label().into(),
        status_code: j.status.index() as i32,
        complaint: j.complaint.clone().into(),
        diagnosis: j.diagnosis.clone().into(),
        odometer: if j.odometer > 0 { j.odometer.to_string().into() } else { "".into() },
        battery: j.battery_note.clone().into(),
        tech_index,
        warranty: j.is_warranty,
        coupon: if j.coupon_seq > 0 { format!("Free service coupon #{} used: the service charge is free.", j.coupon_seq).into() } else { "".into() },
        created: age(&j.created_at).into(),
        parts_total: ctx.money(j.parts_total),
        labor_total: ctx.money(j.labor_total),
        total: ctx.money(j.total()),
        open: j.status.is_open(),
        invoice: j.invoice_no.clone().into(),
        next_date: next.into(),
    });
    d.set_job_items(model(
        j.items
            .iter()
            .map(|it| JobItemRow {
                id: it.id as i32,
                code: it.code.clone().into(),
                desc: it.description.clone().into(),
                qty: it.qty as i32,
                price: ctx.money(it.unit_price),
                total: if it.free { ctx.money(0) } else { ctx.money(it.charge()) },
                free: it.free,
                is_part: it.kind == JobItemKind::Part,
            })
            .collect(),
    ));
    d.set_job_error("".into());
    d.set_show_job(true);
}

fn current_job(ctx: &Ctx) -> i64 {
    let _ = ctx;
    ST.with(|st| st.borrow().job_id)
}

fn job_err(ctx: &Ctx, e: impl std::fmt::Display) {
    ctx.ui().global::<WorkshopData>().set_job_error(e.to_string().into());
}

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let d = ui.global::<WorkshopData>();

    {
        let ctx = ctx.clone();
        d.on_refresh(move || refresh(&ctx));
    }
    {
        let ctx = ctx.clone();
        d.on_open_new(move || {
            load_lists(&ctx);
            let ui = ctx.ui();
            let d = ui.global::<WorkshopData>();
            d.set_bike_hits(model(vec![]));
            d.set_new_bike_id(0);
            d.set_new_coupons(0);
            d.set_new_error("".into());
            d.set_show_new(true);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_search_bikes(move |q| {
            let bikes = ctx.pos.find_vehicles(&q).unwrap_or_default();
            ctx.ui().global::<WorkshopData>().set_bike_hits(model(
                bikes
                    .iter()
                    .map(|v| BikeHit {
                        id: v.id as i32,
                        label: format!(
                            "{}{}",
                            v.label(),
                            if v.color.is_empty() { String::new() } else { format!(" · {}", v.color) }
                        )
                        .into(),
                        customer: if v.customer_name.is_empty() { "No owner recorded".into() } else { v.customer_name.clone().into() },
                        phone: v.customer_phone.clone().into(),
                        coupons: v.coupons_left as i32,
                    })
                    .collect(),
            ));
        });
    }
    {
        let ctx = ctx.clone();
        d.on_pick_bike(move |id| pick_bike(&ctx, id as i64));
    }
    {
        let ctx = ctx.clone();
        d.on_add_own_bike(move |cust, model_name, chassis, reg, color| {
            if cust <= 0 {
                ctx.ui().global::<WorkshopData>().set_new_error("Please choose the customer.".into());
                return false;
            }
            match ctx.pos.add_customer_bike(cust as i64, &model_name, &chassis, &reg, &color) {
                Ok(id) => {
                    pick_bike(&ctx, id);
                    true
                }
                Err(e) => {
                    ctx.ui().global::<WorkshopData>().set_new_error(e.to_string().into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_create_job(move |f: NewJobForm| {
            let ui = ctx.ui();
            let d = ui.global::<WorkshopData>();
            let bike = ST.with(|st| st.borrow().new_bike);
            let r = (|| -> Result<i64, String> {
                let tech = ST.with(|st| st.borrow().techs.get(f.tech_index.max(-1) as usize).map(|u| u.id)).filter(|_| f.tech_index >= 0);
                ctx.pos
                    .create_job(JobInput {
                        vehicle_id: bike,
                        customer_id: None,
                        odometer: int_in(&f.odometer, "Odometer")?,
                        battery_note: f.battery.to_string(),
                        complaint: f.complaint.to_string(),
                        technician_id: tech,
                        is_warranty: f.warranty,
                        use_coupon: f.coupon,
                    })
                    .map_err(|e| e.to_string())
            })();
            match r {
                Ok(id) => {
                    d.set_show_new(false);
                    ctx.ok("Job card opened");
                    refresh(&ctx);
                    show_job(&ctx, id);
                    true
                }
                Err(e) => {
                    d.set_new_error(e.into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_open_job(move |id| {
            load_lists(&ctx);
            show_job(&ctx, id as i64);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_set_status(move |code| {
            let Some(s) = JobStatus::from_index(code as usize) else { return };
            let id = current_job(&ctx);
            match ctx.pos.set_job_status(id, s) {
                Ok(()) => {
                    show_job(&ctx, id);
                    refresh(&ctx);
                }
                Err(e) => job_err(&ctx, e),
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_save_job(move |e: JobEdit| {
            let id = current_job(&ctx);
            let Ok(j) = ctx.pos.get_job(id) else { return false };
            if !j.status.is_open() {
                return true;
            }
            let r = (|| -> Result<(), String> {
                let tech = ST.with(|st| st.borrow().techs.get(e.tech_index.max(-1) as usize).map(|u| u.id)).filter(|_| e.tech_index >= 0);
                ctx.pos
                    .update_job(
                        id,
                        JobUpdate {
                            odometer: int_in(&e.odometer, "Odometer")?,
                            battery_note: e.battery.to_string(),
                            complaint: e.complaint.to_string(),
                            diagnosis: e.diagnosis.to_string(),
                            technician_id: tech,
                            is_warranty: e.warranty,
                            approved: j.approved,
                            next_service_date: j.next_service_date.clone(),
                            next_service_km: j.next_service_km,
                        },
                    )
                    .map_err(|e| e.to_string())
            })();
            match r {
                Ok(()) => {
                    refresh(&ctx);
                    true
                }
                Err(e) => {
                    job_err(&ctx, e);
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_add_part(move |pid| {
            let id = current_job(&ctx);
            match ctx.pos.add_job_part(id, pid as i64, 1, false) {
                Ok(()) => {
                    show_job(&ctx, id);
                    refresh(&ctx);
                    ctx.update_badges();
                }
                Err(e) => job_err(&ctx, e),
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_add_service(move |idx, desc, price| {
            let id = current_job(&ctx);
            let svc = ST.with(|st| st.borrow().services.get(idx.max(-1) as usize).map(|s| s.id)).filter(|_| idx >= 0);
            let price = match money_in(&price, "Price") {
                Ok(p) => p,
                Err(e) => return job_err(&ctx, e),
            };
            if svc.is_none() && desc.trim().is_empty() {
                return job_err(&ctx, "Choose a service from the list or describe the work.");
            }
            match ctx.pos.add_job_service(id, svc, &desc, price, false) {
                Ok(()) => {
                    show_job(&ctx, id);
                    refresh(&ctx);
                }
                Err(e) => job_err(&ctx, e),
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_remove_item(move |item| {
            let id = current_job(&ctx);
            match ctx.pos.remove_job_item(item as i64) {
                Ok(()) => {
                    show_job(&ctx, id);
                    refresh(&ctx);
                    ctx.update_badges();
                }
                Err(e) => job_err(&ctx, e),
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_toggle_free(move |item, free| {
            let id = current_job(&ctx);
            match ctx.pos.set_job_item_free(item as i64, free) {
                Ok(()) => show_job(&ctx, id),
                Err(e) => job_err(&ctx, e),
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_print_job(move || ctx.print_job(current_job(&ctx)));
    }
    {
        let ctx = ctx.clone();
        d.on_print_invoice(move || {
            if let Ok(j) = ctx.pos.get_job(current_job(&ctx)) {
                if let Some(sid) = j.sale_id {
                    ctx.print_sale(sid, crate::print::Paper::A4);
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_cancel_job(move || {
            let id = current_job(&ctx);
            match ctx.pos.set_job_status(id, JobStatus::Cancelled) {
                Ok(()) => {
                    ctx.ui().global::<WorkshopData>().set_show_job(false);
                    ctx.ok("Job cancelled. Parts went back to stock.");
                    refresh(&ctx);
                    ctx.update_badges();
                }
                Err(e) => job_err(&ctx, e),
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_open_deliver(move || {
            let id = current_job(&ctx);
            let Ok(j) = ctx.pos.get_job(id) else { return };
            let lines: Vec<pos_core::CartLine> = j
                .items
                .iter()
                .map(|it| pos_core::CartLine {
                    item_type: pos_core::ItemType::Custom,
                    ref_id: None,
                    code: String::new(),
                    description: String::new(),
                    qty: it.qty,
                    unit_price: it.unit_price,
                    line_discount: 0,
                    serial_no: String::new(),
                    free: it.free,
                })
                .collect();
            let total = ctx.pos.totals(&lines, 0).total;
            let ui = ctx.ui();
            let d = ui.global::<WorkshopData>();
            d.set_deliver_total(ctx.money(total));
            d.set_deliver_total_plain(ctx.pos.money().edit(total).into());
            d.set_deliver_error("".into());
            d.set_show_deliver(true);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_deliver(move |f: DeliverForm| {
            let ui = ctx.ui();
            let d = ui.global::<WorkshopData>();
            let id = current_job(&ctx);
            let r = (|| -> Result<pos_core::SaleReceipt, String> {
                let discount = money_in(&f.discount, "Discount")?;
                let cash = money_in(&f.cash, "Cash")?;
                let bank = money_in(&f.bank, "Bank")?;
                let next = f.next_date.trim().to_string();
                if !next.is_empty() && pos_core::util::parse_date(&next).is_none() {
                    return Err("Next service date must look like 2026-12-31.".into());
                }
                let mut payments = Vec::new();
                if cash > 0 {
                    payments.push(PaymentInput::cash(cash));
                }
                if bank > 0 {
                    payments.push(PaymentInput::bank(bank, &f.bank_name, &f.reference));
                }
                ctx.pos
                    .deliver_job(id, discount, payments, &next, int_in(&f.next_km, "Next service km")?)
                    .map_err(|e| e.to_string())
            })();
            match r {
                Ok(r) => {
                    d.set_show_deliver(false);
                    d.set_show_job(false);
                    let msg = if r.balance > 0 {
                        format!("Delivered · {} · {} left on account", r.invoice_no, ctx.money(r.balance))
                    } else {
                        format!("Delivered · invoice {}", r.invoice_no)
                    };
                    ctx.ok(msg);
                    refresh(&ctx);
                    true
                }
                Err(e) => {
                    d.set_deliver_error(e.into());
                    false
                }
            }
        });
    }
}

fn pick_bike(ctx: &Rc<Ctx>, id: i64) {
    let Ok(v) = ctx.pos.get_vehicle(id) else { return };
    ST.with(|st| st.borrow_mut().new_bike = v.id);
    let ui = ctx.ui();
    let d = ui.global::<WorkshopData>();
    d.set_new_bike_label(
        format!("{}{}", v.label(), if v.color.is_empty() { String::new() } else { format!(" · {}", v.color) }).into(),
    );
    d.set_new_bike_customer(
        if v.customer_name.is_empty() {
            "No owner recorded".to_string()
        } else {
            format!("{} {}", v.customer_name, v.customer_phone)
        }
        .into(),
    );
    d.set_new_coupons(v.coupons_left as i32);
    d.set_new_error("".into());
    d.set_new_bike_id(v.id as i32);
}
