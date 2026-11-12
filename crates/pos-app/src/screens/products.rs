//! Parts & stock screen.

use crate::conv::{int_in, level_num, model, money_in, strings};
use crate::ctx::Ctx;
use crate::{CatRow, MoveRow, ProductForm, ProductRow, ProductsData, ReceiveLine as UiReceiveLine};
use pos_core::money::{margin_bp, parse_money, fmt_percent};
use pos_core::util::pretty_ts;
use pos_core::{Category, LevelFilter, ProductFilter, ProductInput, ProductKind, ReceiveInput, ReceiveLine, Supplier};
use slint::{ComponentHandle, Model, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct State {
    categories: Vec<Category>,
    suppliers: Vec<Supplier>,
    adjust_id: i64,
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State::default());
}

fn category_id_at(index: i32) -> Option<i64> {
    ST.with(|st| st.borrow().categories.get(index.max(-1) as usize).map(|c| c.id)).filter(|_| index >= 0)
}

fn load_lists(ctx: &Ctx) {
    let cats = ctx.pos.list_categories().unwrap_or_default();
    let sups = ctx.pos.list_suppliers().unwrap_or_default();
    let ui = ctx.ui();
    let d = ui.global::<ProductsData>();
    d.set_categories(model(
        cats.iter()
            .map(|c| CatRow { id: c.id as i32, name: c.name.clone().into(), prefix: c.prefix.clone().into(), count: c.product_count as i32 })
            .collect(),
    ));
    d.set_category_names(strings(cats.iter().map(|c| c.name.clone()).collect()));
    let mut f = vec!["All categories".to_string()];
    f.extend(cats.iter().map(|c| c.name.clone()));
    d.set_filter_names(strings(f));
    d.set_supplier_names(strings(sups.iter().map(|s| s.name.clone()).collect()));
    ST.with(|st| {
        let mut st = st.borrow_mut();
        st.categories = cats;
        st.suppliers = sups;
    });
}

pub fn refresh(ctx: &Rc<Ctx>) {
    load_lists(ctx);
    let ui = ctx.ui();
    let d = ui.global::<ProductsData>();
    let cat_filter = d.get_category_filter();
    let filter = ProductFilter {
        search: d.get_search().to_string(),
        category_id: if cat_filter > 0 { category_id_at(cat_filter - 1) } else { None },
        level: match d.get_level_filter() {
            1 => LevelFilter::Low,
            2 => LevelFilter::Out,
            _ => LevelFilter::All,
        },
        include_inactive: true,
        ..Default::default()
    };
    let products = match ctx.pos.list_products(&filter) {
        Ok(p) => p,
        Err(e) => {
            ctx.err(e);
            return;
        }
    };
    let rows: Vec<ProductRow> = products
        .iter()
        .map(|p| {
            let mut sub = Vec::new();
            if !p.fits_models.is_empty() {
                sub.push(format!("Fits {}", p.fits_models));
            }
            if !p.brand.is_empty() {
                sub.push(p.brand.clone());
            }
            if !p.location.is_empty() {
                sub.push(p.location.clone());
            }
            if sub.is_empty() {
                sub.push(p.kind.label().to_string());
            }
            ProductRow {
                id: p.id as i32,
                code: p.code.clone().into(),
                name: p.name.clone().into(),
                sub: sub.join(" · ").into(),
                category: if p.category_name.is_empty() { "—".into() } else { p.category_name.clone().into() },
                kind: p.kind.label().into(),
                stock: p.stock as i32,
                low: p.low_stock as i32,
                level: level_num(p.level()),
                unit: p.unit.clone().into(),
                cost: ctx.money(p.cost),
                price: ctx.money(p.price),
                margin: if p.price > 0 { fmt_percent(margin_bp(p.price - p.cost, p.price)).into() } else { "—".into() },
                active: p.active,
            }
        })
        .collect();
    d.set_rows(model(rows));
    if let Ok(r) = ctx.pos.stock_report() {
        d.set_stat_products(r.products.to_string().into());
        d.set_stat_units(r.units.to_string().into());
        d.set_stat_value(ctx.money(r.value_cost));
        d.set_stat_retail(ctx.money(r.value_retail));
        d.set_stat_low(r.low as i32);
        d.set_stat_out(r.out as i32);
    }
}

fn form_from(ctx: &Ctx, p: Option<&pos_core::Product>) -> ProductForm {
    let fmt = ctx.pos.money();
    let cat_index = |id: Option<i64>| -> i32 {
        ST.with(|st| {
            id.and_then(|id| st.borrow().categories.iter().position(|c| c.id == id)).map(|i| i as i32).unwrap_or(-1)
        })
    };
    match p {
        Some(p) => ProductForm {
            id: p.id as i32,
            code: p.code.clone().into(),
            name: p.name.clone().into(),
            category: cat_index(p.category_id),
            kind: p.kind.index() as i32,
            brand: p.brand.clone().into(),
            unit: p.unit.clone().into(),
            cost: fmt.edit(p.cost).into(),
            price: fmt.edit(p.price).into(),
            stock: p.stock.to_string().into(),
            low: p.low_stock.to_string().into(),
            fits: p.fits_models.clone().into(),
            location: p.location.clone().into(),
            warranty: if p.warranty_months > 0 { p.warranty_months.to_string().into() } else { SharedString::new() },
            barcode: p.barcode.clone().into(),
            notes: p.notes.clone().into(),
            active: p.active,
            serial: p.serial_tracked,
        },
        None => ProductForm {
            id: 0,
            category: -1,
            kind: 0,
            unit: "pcs".into(),
            low: ctx.pos.settings().low_stock_default.to_string().into(),
            active: true,
            code: if ctx.pos.settings().suggest_codes { ctx.pos.suggest_code(None).unwrap_or_default().into() } else { SharedString::new() },
            ..Default::default()
        },
    }
}

thread_local! {
    /// Lines in the "Receive stock" dialog. Rows are updated in place so typing isn't interrupted.
    static RECEIVE: Rc<VecModel<UiReceiveLine>> = Rc::new(VecModel::default());
}

fn receive_model(_ctx: &Ctx) -> Rc<VecModel<UiReceiveLine>> {
    RECEIVE.with(|m| m.clone())
}

fn update_receive_total(ctx: &Ctx) {
    let m = receive_model(ctx);
    let total: i64 = m
        .iter()
        .map(|l| parse_money(&l.cost).unwrap_or(0) * l.qty.trim().parse::<i64>().unwrap_or(0))
        .sum();
    ctx.ui().global::<ProductsData>().set_receive_total(ctx.money(total));
}

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let d = ui.global::<ProductsData>();
    d.set_receive_lines(slint::ModelRc::from(receive_model(ctx)));

    {
        let ctx = ctx.clone();
        d.on_refresh(move || refresh(&ctx));
    }
    {
        let ctx = ctx.clone();
        d.on_new_product(move || {
            load_lists(&ctx);
            let ui = ctx.ui();
            let d = ui.global::<ProductsData>();
            d.set_form(form_from(&ctx, None));
            d.set_form_error("".into());
            d.set_show_form(true);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_edit(move |id| {
            load_lists(&ctx);
            match ctx.pos.get_product(id as i64) {
                Ok(p) => {
                    let ui = ctx.ui();
                    let d = ui.global::<ProductsData>();
                    d.set_form(form_from(&ctx, Some(&p)));
                    d.set_form_error("".into());
                    d.set_show_form(true);
                }
                Err(e) => ctx.err(e),
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_suggest_code(move |idx| ctx.pos.suggest_code(category_id_at(idx)).unwrap_or_default().into());
    }
    {
        let ctx = ctx.clone();
        d.on_margin_text(move |cost, price| -> SharedString {
            let (Some(c), Some(p)) = (parse_money(&cost), parse_money(&price)) else { return SharedString::new() };
            if p <= 0 {
                return SharedString::new();
            }
            let profit = p - c;
            format!("Profit {} per item ({} margin)", ctx.money(profit), fmt_percent(margin_bp(profit, p))).into()
        });
    }
    {
        let ctx = ctx.clone();
        d.on_save(move |f| {
            let ui = ctx.ui();
            let d = ui.global::<ProductsData>();
            let r = (|| -> Result<i64, String> {
                let existing = if f.id > 0 { ctx.pos.get_product(f.id as i64).ok() } else { None };
                let cost = if ctx.pos.can(pos_core::Perm::ViewCost) {
                    money_in(&f.cost, "Cost price")?
                } else {
                    existing.as_ref().map(|p| p.cost).unwrap_or(0)
                };
                let mut code = f.code.to_string();
                if code.trim().is_empty() {
                    code = ctx.pos.suggest_code(category_id_at(f.category)).map_err(|e| e.to_string())?;
                }
                let input = ProductInput {
                    id: if f.id > 0 { Some(f.id as i64) } else { None },
                    code,
                    barcode: f.barcode.to_string(),
                    name: f.name.to_string(),
                    category_id: category_id_at(f.category),
                    kind: ProductKind::from_index(f.kind.max(0) as usize).unwrap_or(ProductKind::Part),
                    brand: f.brand.to_string(),
                    unit: f.unit.to_string(),
                    cost,
                    price: money_in(&f.price, "Sale price")?,
                    stock: int_in(&f.stock, "Opening stock")?,
                    low_stock: int_in(&f.low, "Low stock level")?,
                    fits_models: f.fits.to_string(),
                    location: f.location.to_string(),
                    serial_tracked: f.serial,
                    warranty_months: int_in(&f.warranty, "Warranty")?,
                    notes: f.notes.to_string(),
                    active: f.active,
                };
                ctx.pos.save_product(input).map_err(|e| e.to_string())
            })();
            match r {
                Ok(_) => {
                    d.set_show_form(false);
                    ctx.ok(if f.id > 0 { "Product updated" } else { "Product added" });
                    refresh(&ctx);
                    ctx.update_badges();
                    true
                }
                Err(e) => {
                    d.set_form_error(e.into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_open_adjust(move |id| {
            if let Ok(p) = ctx.pos.get_product(id as i64) {
                ST.with(|st| st.borrow_mut().adjust_id = p.id);
                let ui = ctx.ui();
                let d = ui.global::<ProductsData>();
                d.set_adjust_name(p.name.into());
                d.set_adjust_code(p.code.into());
                d.set_adjust_stock(p.stock as i32);
                d.set_adjust_unit(p.unit.into());
                d.set_adjust_error("".into());
                d.set_show_adjust(true);
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_adjust(move |mode, qty, reason, note| {
            let ui = ctx.ui();
            let d = ui.global::<ProductsData>();
            let id = ST.with(|st| st.borrow().adjust_id);
            let n = match int_in(&qty, "Quantity") {
                Ok(n) if n >= 0 => n,
                Ok(_) => {
                    d.set_adjust_error("Enter a positive number.".into());
                    return false;
                }
                Err(e) => {
                    d.set_adjust_error(e.into());
                    return false;
                }
            };
            let r = match mode {
                0 => ctx.pos.adjust_stock(id, n, &reason, &note).map(|_| ()),
                1 => ctx.pos.adjust_stock(id, -n, &reason, &note).map(|_| ()),
                _ => ctx.pos.stocktake(id, n, &note).map(|_| ()),
            };
            match r {
                Ok(()) => {
                    d.set_show_adjust(false);
                    ctx.ok("Stock updated");
                    refresh(&ctx);
                    ctx.update_badges();
                    true
                }
                Err(e) => {
                    d.set_adjust_error(e.to_string().into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_open_receive(move || {
            load_lists(&ctx);
            let m = receive_model(&ctx);
            m.set_vec(vec![]);
            let ui = ctx.ui();
            let d = ui.global::<ProductsData>();
            d.set_receive_error("".into());
            update_receive_total(&ctx);
            d.set_show_receive(true);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_receive_add(move |id| {
            let Ok(p) = ctx.pos.get_product(id as i64) else { return };
            let m = receive_model(&ctx);
            if let Some(i) = m.iter().position(|l| l.product_id == p.id as i32) {
                let mut l = m.row_data(i).unwrap();
                let q = l.qty.trim().parse::<i64>().unwrap_or(0) + 1;
                l.qty = q.to_string().into();
                l.total = ctx.money(parse_money(&l.cost).unwrap_or(0) * q);
                m.set_row_data(i, l);
            } else {
                m.push(UiReceiveLine {
                    product_id: p.id as i32,
                    code: p.code.clone().into(),
                    name: p.name.clone().into(),
                    qty: "1".into(),
                    cost: ctx.pos.money().edit(p.cost).into(),
                    total: ctx.money(p.cost),
                });
            }
            update_receive_total(&ctx);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_receive_set(move |i, qty, cost| {
            let m = receive_model(&ctx);
            if let Some(mut l) = m.row_data(i as usize) {
                l.qty = qty.clone();
                l.cost = cost.clone();
                let q = qty.trim().parse::<i64>().unwrap_or(0);
                l.total = ctx.money(parse_money(&cost).unwrap_or(0) * q);
                m.set_row_data(i as usize, l);
            }
            update_receive_total(&ctx);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_receive_remove(move |i| {
            let m = receive_model(&ctx);
            if (i as usize) < m.row_count() {
                m.remove(i as usize);
            }
            update_receive_total(&ctx);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_receive_save(move |supplier, notes| {
            let ui = ctx.ui();
            let d = ui.global::<ProductsData>();
            let m = receive_model(&ctx);
            let mut items = Vec::new();
            for l in m.iter() {
                let qty = match l.qty.trim().parse::<i64>() {
                    Ok(q) => q,
                    Err(_) => {
                        d.set_receive_error(format!("Quantity for {} must be a whole number.", l.code).into());
                        return false;
                    }
                };
                let Some(cost) = parse_money(&l.cost) else {
                    d.set_receive_error(format!("Cost for {} must be an amount.", l.code).into());
                    return false;
                };
                items.push(ReceiveLine { product_id: l.product_id as i64, qty, unit_cost: cost });
            }
            let supplier_id = ST.with(|st| st.borrow().suppliers.get(supplier.max(-1) as usize).map(|s| s.id)).filter(|_| supplier >= 0);
            match ctx.pos.receive_stock(ReceiveInput { supplier_id, items, notes: notes.to_string() }) {
                Ok(no) => {
                    d.set_show_receive(false);
                    ctx.ok(format!("Stock received ({no})"));
                    refresh(&ctx);
                    ctx.update_badges();
                    true
                }
                Err(e) => {
                    d.set_receive_error(e.to_string().into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_add_supplier(move |name, phone| {
            match ctx.pos.save_supplier(Supplier { name: name.to_string(), phone: phone.to_string(), ..Default::default() }) {
                Ok(id) => {
                    load_lists(&ctx);
                    ST.with(|st| st.borrow().suppliers.iter().position(|s| s.id == id).map(|i| i as i32).unwrap_or(-1))
                }
                Err(e) => {
                    ctx.ui().global::<ProductsData>().set_receive_error(e.to_string().into());
                    -1
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_save_category(move |id, name, prefix| {
            let ui = ctx.ui();
            let d = ui.global::<ProductsData>();
            match ctx.pos.save_category(if id > 0 { Some(id as i64) } else { None }, &name, &prefix) {
                Ok(_) => {
                    d.set_cat_error("".into());
                    load_lists(&ctx);
                    refresh(&ctx);
                    true
                }
                Err(e) => {
                    d.set_cat_error(e.to_string().into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_delete_category(move |id| {
            if let Err(e) = ctx.pos.delete_category(id as i64) {
                ctx.err(e);
            }
            refresh(&ctx);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_open_history(move |id| {
            let Ok(p) = ctx.pos.get_product(id as i64) else { return };
            let moves = ctx.pos.product_movements(p.id, 300).unwrap_or_default();
            let rows: Vec<MoveRow> = moves
                .iter()
                .map(|m| MoveRow {
                    date: pretty_ts(&m.created_at).into(),
                    change: format!("{:+}", m.change).into(),
                    positive: m.change > 0,
                    reason: reason_label(&m.reason).into(),
                    reference: if m.note.is_empty() { m.reference.clone() } else if m.reference.is_empty() { m.note.clone() } else { format!("{} · {}", m.reference, m.note) }.into(),
                    user: m.user_name.clone().into(),
                })
                .collect();
            let ui = ctx.ui();
            let d = ui.global::<ProductsData>();
            d.set_history_title(format!("{} · {} · now {} {}", p.code, p.name, p.stock, p.unit).into());
            d.set_history(model(rows));
            d.set_show_history(true);
        });
    }
}

fn reason_label(r: &str) -> String {
    match r {
        "sale" => "Sold".into(),
        "return" => "Customer return".into(),
        "purchase" => "Received from supplier".into(),
        "opening" => "Opening stock".into(),
        "job" => "Used in workshop job".into(),
        "job_return" => "Removed from job".into(),
        "job_cancel" => "Job cancelled".into(),
        "stocktake" => "Stocktake".into(),
        other => {
            let mut c = other.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        }
    }
}
