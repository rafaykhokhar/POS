//! Home dashboard.

use crate::conv::{level_num, model, sale_row};
use crate::ctx::Ctx;
use crate::{Bar, DashData, DashStats, DueRow, LowRow, Page, StatusCount};
use chrono::Timelike;
use pos_core::util::pretty_date;
use slint::ComponentHandle;
use std::rc::Rc;

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let d = ui.global::<DashData>();
    {
        let ctx = ctx.clone();
        d.on_refresh(move || refresh(&ctx));
    }
    {
        let ctx = ctx.clone();
        d.on_open_low_stock(move || {
            ctx.ui().global::<crate::ProductsData>().set_level_filter(1);
            super::navigate(&ctx, Page::Products);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_open_sale(move |id| {
            if ctx.pos.can(pos_core::Perm::Reports) {
                super::navigate(&ctx, Page::Reports);
                super::reports::show_sale(&ctx, id as i64);
            }
        });
    }
}

pub fn refresh(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let d = ui.global::<DashData>();
    let hour = chrono::Local::now().hour();
    let part = if hour < 12 {
        "Good morning"
    } else if hour < 17 {
        "Good afternoon"
    } else {
        "Good evening"
    };
    let first = ctx.pos.current_user().map(|u| u.name.split_whitespace().next().unwrap_or("").to_string()).unwrap_or_default();
    d.set_greeting(format!("{part}, {first}").into());

    let dash = match ctx.pos.dashboard() {
        Ok(x) => x,
        Err(e) => {
            ctx.err(e);
            return;
        }
    };
    d.set_stats(DashStats {
        sales_today: ctx.money(dash.sales_today),
        count_today: dash.count_today.to_string().into(),
        profit_today: ctx.money(dash.profit_today),
        cash_today: ctx.money(dash.cash_today),
        bank_today: ctx.money(dash.bank_today),
        sales_month: ctx.money(dash.sales_month),
        profit_month: ctx.money(dash.profit_month),
        vehicles_stock: dash.vehicles_in_stock.to_string().into(),
        vehicles_sold: dash.vehicles_sold_month.to_string().into(),
        open_jobs: dash.open_jobs.to_string().into(),
        ready_jobs: dash.ready_jobs.to_string().into(),
        low_count: dash.low_stock_count as i32,
        out_count: dash.out_of_stock_count as i32,
        receivables: ctx.money(dash.receivables),
        bookings: dash.open_bookings.to_string().into(),
    });
    let max = dash.last7.iter().map(|b| b.total).max().unwrap_or(0).max(1);
    let today = pos_core::util::today();
    let fmt = ctx.pos.money();
    let bars: Vec<Bar> = dash
        .last7
        .iter()
        .map(|b| Bar {
            label: b.label.clone().into(),
            value: if b.total == 0 { "".into() } else { compact(&fmt, b.total).into() },
            ratio: (b.total.max(0) as f64 / max as f64) as f32,
            today: b.date == today,
        })
        .collect();
    d.set_bars(model(bars));
    d.set_week_total(ctx.money(dash.last7.iter().map(|b| b.total).sum()));
    d.set_recent(model(dash.recent_sales.iter().map(|s| sale_row(ctx, s)).collect()));
    d.set_low(model(
        dash.low_stock
            .iter()
            .map(|p| LowRow {
                id: p.id as i32,
                code: p.code.clone().into(),
                name: p.name.clone().into(),
                stock: p.stock as i32,
                low: p.low_stock as i32,
                level: level_num(p.level()),
                unit: p.unit.clone().into(),
            })
            .collect(),
    ));
    let jmax = dash.jobs_by_status.iter().map(|(_, n)| *n).max().unwrap_or(1).max(1);
    d.set_jobs_max(jmax as i32);
    d.set_jobs(model(
        dash.jobs_by_status
            .iter()
            .enumerate()
            .map(|(i, (s, n))| StatusCount { label: s.label().into(), count: *n as i32, index: i as i32 })
            .collect(),
    ));
    d.set_due(model(
        dash.service_due
            .iter()
            .map(|s| DueRow {
                vehicle: s.vehicle_label.clone().into(),
                customer: if s.customer_name.is_empty() { "Unknown".into() } else { s.customer_name.clone().into() },
                phone: s.customer_phone.clone().into(),
                due: if s.overdue { format!("Overdue · {}", pretty_date(&s.due_date)) } else { pretty_date(&s.due_date) }.into(),
                overdue: s.overdue,
            })
            .collect(),
    ));
}

/// Short money for chart labels: 12.5k, 1.2M.
pub fn compact(fmt: &pos_core::MoneyFmt, m: i64) -> String {
    let units = m as f64 / 100.0;
    let a = units.abs();
    let s = if a >= 1_000_000.0 {
        format!("{:.1}M", units / 1_000_000.0)
    } else if a >= 1_000.0 {
        format!("{:.1}k", units / 1_000.0)
    } else {
        format!("{:.0}", units)
    };
    let _ = fmt;
    s.replace(".0k", "k").replace(".0M", "M")
}
