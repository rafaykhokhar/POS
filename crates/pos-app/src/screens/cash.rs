//! Cash drawer.

use crate::conv::{model, money_in};
use crate::ctx::Ctx;
use crate::{CashData, CashMoveRow, ShiftRow, KV};
use pos_core::money::parse_money;
use pos_core::util::{pretty_time, pretty_ts};
use slint::{ComponentHandle, SharedString};
use std::rc::Rc;

pub fn refresh(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let c = ui.global::<CashData>();
    match ctx.pos.current_shift() {
        Ok(Some(s)) => {
            c.set_open(true);
            c.set_opened_info(format!("Opened {} by {}", pretty_ts(&s.opened_at), s.opened_by).into());
            c.set_expected(ctx.money(s.expected));
            let kv = |label: &str, value: String, tone: i32| KV { label: label.into(), value: value.into(), tone };
            c.set_summary(model(vec![
                kv("Opening cash", ctx.money(s.opening_cash).to_string(), 0),
                kv("Cash from sales (after refunds)", ctx.money(s.cash_sales).to_string(), 2),
                kv("Cash put in", ctx.money(s.cash_in).to_string(), 0),
                kv("Cash taken out", format!("− {}", ctx.money(s.cash_out)), if s.cash_out > 0 { 1 } else { 0 }),
                kv("Bank transfers (not in drawer)", ctx.money(s.bank_total).to_string(), 0),
            ]));
            let moves = ctx.pos.shift_moves(s.id).unwrap_or_default();
            c.set_moves(model(
                moves
                    .iter()
                    .map(|m| CashMoveRow {
                        reason: m.reason.clone().into(),
                        user: m.user_name.clone().into(),
                        time: pretty_time(&m.created_at).into(),
                        amount: if m.amount > 0 { format!("+ {}", ctx.money(m.amount)) } else { format!("− {}", ctx.money(-m.amount)) }.into(),
                        positive: m.amount > 0,
                    })
                    .collect(),
            ));
        }
        Ok(None) => c.set_open(false),
        Err(e) => ctx.err(e),
    }
    let shifts = ctx.pos.list_shifts(60).unwrap_or_default();
    c.set_history(model(
        shifts
            .iter()
            .filter(|s| !s.is_open())
            .map(|s| {
                let diff = s.difference().unwrap_or(0);
                ShiftRow {
                    opened: pretty_ts(&s.opened_at).into(),
                    closed: pretty_ts(&s.closed_at).into(),
                    by: s.closed_by.clone().into(),
                    expected: ctx.money(s.expected),
                    counted: s.counted.map(|c| ctx.money(c)).unwrap_or_default(),
                    diff: if diff == 0 { "Exact ✓".into() } else if diff > 0 { format!("+ {}", ctx.money(diff)).into() } else { format!("− {}", ctx.money(-diff)).into() },
                    diff_tone: if diff < 0 { 1 } else if diff > 0 { 2 } else { 0 },
                }
            })
            .collect(),
    ));
    ctx.update_badges();
}

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let c = ui.global::<CashData>();
    {
        let ctx = ctx.clone();
        c.on_refresh(move || refresh(&ctx));
    }
    {
        let ctx = ctx.clone();
        c.on_open_drawer(move |amount| {
            let ui = ctx.ui();
            let c = ui.global::<CashData>();
            let r = money_in(&amount, "Opening cash").and_then(|a| ctx.pos.open_shift(a).map_err(|e| e.to_string()));
            match r {
                Ok(_) => {
                    c.set_error("".into());
                    ctx.ok("Cash drawer opened");
                    refresh(&ctx);
                    true
                }
                Err(e) => {
                    c.set_error(e.into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        c.on_cash_move(move |dir, amount, reason| {
            let ui = ctx.ui();
            let c = ui.global::<CashData>();
            let r = money_in(&amount, "Amount").and_then(|a| {
                if a <= 0 {
                    return Err("Enter an amount.".to_string());
                }
                ctx.pos.cash_move(if dir > 0 { a } else { -a }, &reason).map_err(|e| e.to_string())
            });
            match r {
                Ok(()) => {
                    c.set_show_move(false);
                    ctx.ok("Saved");
                    refresh(&ctx);
                    true
                }
                Err(e) => {
                    c.set_error(e.into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        c.on_diff_text(move |counted| -> SharedString {
            let Some(cnt) = parse_money(&counted) else { return "Enter a valid amount".into() };
            let expected = ctx.pos.current_shift().ok().flatten().map(|s| s.expected).unwrap_or(0);
            let diff = cnt - expected;
            if diff == 0 {
                "Exactly right ✓".into()
            } else if diff > 0 {
                format!("{} extra in the drawer", ctx.money(diff)).into()
            } else {
                format!("{} short", ctx.money(-diff)).into()
            }
        });
    }
    {
        let ctx = ctx.clone();
        c.on_close_drawer(move |counted, notes| {
            let ui = ctx.ui();
            let c = ui.global::<CashData>();
            let r = money_in(&counted, "Counted cash").and_then(|a| ctx.pos.close_shift(a, &notes).map_err(|e| e.to_string()));
            match r {
                Ok(s) => {
                    c.set_show_close(false);
                    let d = s.difference().unwrap_or(0);
                    ctx.ok(if d == 0 {
                        "Drawer closed. Cash was exactly right.".to_string()
                    } else if d > 0 {
                        format!("Drawer closed. {} extra.", ctx.money(d))
                    } else {
                        format!("Drawer closed. {} short.", ctx.money(-d))
                    });
                    refresh(&ctx);
                    true
                }
                Err(e) => {
                    c.set_error(e.into());
                    false
                }
            }
        });
    }
}
