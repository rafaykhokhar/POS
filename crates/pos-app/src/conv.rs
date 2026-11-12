//! Conversions from business types to the UI's display structs.

use crate::ctx::Ctx;
use crate::{CustomerHit, ProductHit, SaleRow};
use pos_core::util::{pretty_date, pretty_time};
use pos_core::{Customer, Product, SaleKind, SaleSummary, StockLevel};
use slint::{ModelRc, SharedString, VecModel};

pub fn initials(name: &str) -> String {
    let parts: Vec<&str> = name.split_whitespace().collect();
    let mut s = String::new();
    for p in parts.iter().take(2) {
        if let Some(c) = p.chars().next() {
            s.extend(c.to_uppercase());
        }
    }
    if s.is_empty() {
        "?".into()
    } else {
        s
    }
}

pub fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

pub fn strings(v: Vec<String>) -> ModelRc<SharedString> {
    model(v.into_iter().map(SharedString::from).collect())
}

pub fn level_num(l: StockLevel) -> i32 {
    match l {
        StockLevel::Ok => 0,
        StockLevel::Warn => 1,
        StockLevel::Low => 2,
        StockLevel::Out => 3,
    }
}

pub fn sale_row(ctx: &Ctx, s: &SaleSummary) -> SaleRow {
    let balance = s.balance();
    SaleRow {
        id: s.id as i32,
        invoice: s.invoice_no.clone().into(),
        kind: s.kind.label().into(),
        is_return: s.kind == SaleKind::Return,
        customer: s.customer_name.clone().into(),
        user: s.user_name.clone().into(),
        total: ctx.money(s.total),
        paid: ctx.money(s.paid),
        balance: ctx.money(balance),
        owing: balance > 0,
        profit: ctx.money(s.profit),
        items: s.items.abs() as i32,
        date: pretty_date(&s.created_at).into(),
        time: pretty_time(&s.created_at).into(),
    }
}

pub fn customer_hit(ctx: &Ctx, c: &Customer) -> CustomerHit {
    CustomerHit {
        id: c.id as i32,
        name: c.name.clone().into(),
        phone: c.phone.clone().into(),
        balance: ctx.money(c.balance),
        owing: c.balance > 0,
        initials: initials(&c.name).into(),
    }
}

pub fn product_hit(ctx: &Ctx, p: &Product) -> ProductHit {
    ProductHit {
        id: p.id as i32,
        code: p.code.clone().into(),
        name: p.name.clone().into(),
        price: ctx.money(p.price),
        stock: p.stock as i32,
        level: level_num(p.level()),
        unit: p.unit.clone().into(),
        info: if p.fits_models.is_empty() { p.category_name.clone().into() } else { p.fits_models.clone().into() },
    }
}

/// Parse a money field; shows a friendly error message on failure.
pub fn money_in(s: &str, what: &str) -> Result<i64, String> {
    pos_core::money::parse_money(s).ok_or_else(|| format!("{what}: please enter a valid amount."))
}

/// Parse a whole number field.
pub fn int_in(s: &str, what: &str) -> Result<i64, String> {
    let t = s.trim().replace(',', "");
    if t.is_empty() {
        return Ok(0);
    }
    t.parse::<i64>().map_err(|_| format!("{what}: please enter a whole number."))
}
