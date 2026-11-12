//! Vehicles: showroom bikes, selling, bookings and models.

use crate::conv::{int_in, model, money_in, strings};
use crate::ctx::Ctx;
use crate::{
    BikeForm, BikeRow, BookingRow, HistRow, InfoPair, ModelCard, ModelForm, SellForm, SellLine, VehiclesData,
};
use pos_core::money::parse_money;
use pos_core::util::{pretty_date, today};
use pos_core::{
    BookingInput, BookingStatus, CartLine, CheckoutInput, ItemType, Money, PayMethod, PaymentInput, Perm, SaleKind,
    Vehicle, VehicleFilter, VehicleInput, VehicleModel, VehicleStatus,
};
use slint::{ComponentHandle, SharedString};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct State {
    models: Vec<VehicleModel>,
    sell_vehicle: i64,
    sell_booking: Option<i64>,
    sell_advance: Money,
    sell_products: Vec<i64>,
    book_vehicle: Option<i64>,
    pending_booking: Option<i64>,
    pay_booking: i64,
    cancel_booking: i64,
    detail_vehicle: i64,
    last_sale: i64,
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State::default());
}

fn status_code(s: VehicleStatus) -> i32 {
    s.index() as i32
}

fn bike_row(ctx: &Ctx, v: &Vehicle, booking: &str) -> BikeRow {
    BikeRow {
        id: v.id as i32,
        model: v.model_name.clone().into(),
        chassis: v.chassis_no.clone().into(),
        motor: if v.motor_no.is_empty() { "—".into() } else { v.motor_no.clone().into() },
        battery: if v.battery_serial.is_empty() { "—".into() } else { v.battery_serial.clone().into() },
        color: v.color.clone().into(),
        year: if v.year > 0 { v.year.to_string().into() } else { SharedString::new() },
        status: v.status.label().into(),
        status_code: status_code(v.status),
        price: ctx.money(v.price),
        cost: ctx.money(v.cost),
        customer: v.customer_name.clone().into(),
        phone: v.customer_phone.clone().into(),
        reg: v.registration_no.clone().into(),
        sold_at: pretty_date(&v.sold_at).into(),
        invoice: v.invoice_no.clone().into(),
        coupons: v.coupons_left as i32,
        booking: booking.into(),
    }
}

fn load_models(ctx: &Ctx) {
    let models = ctx.pos.list_vehicle_models(false).unwrap_or_default();
    let ui = ctx.ui();
    let d = ui.global::<VehiclesData>();
    d.set_model_names(strings(models.iter().map(|m| m.name.clone()).collect()));
    d.set_models(model(
        models
            .iter()
            .map(|m| ModelCard {
                id: m.id as i32,
                name: m.name.clone().into(),
                brand: m.brand.clone().into(),
                price: ctx.money(m.price),
                in_stock: m.in_stock as i32,
                warranty: format!(
                    "Warranty: vehicle {} mo · battery {} mo · motor {} mo{}",
                    m.vehicle_warranty_months,
                    m.battery_warranty_months,
                    m.motor_warranty_months,
                    if m.warranty_km > 0 { format!(" · up to {} km", m.warranty_km) } else { String::new() }
                )
                .into(),
                services: if m.free_services > 0 {
                    format!("{} free services with every bike", m.free_services).into()
                } else {
                    "No free services".into()
                },
            })
            .collect(),
    ));
    ST.with(|st| st.borrow_mut().models = models);
}

fn model_index(id: Option<i64>) -> i32 {
    ST.with(|st| id.and_then(|id| st.borrow().models.iter().position(|m| m.id == id)).map(|i| i as i32).unwrap_or(-1))
}

fn model_at(i: i32) -> Option<VehicleModel> {
    if i < 0 {
        return None;
    }
    ST.with(|st| st.borrow().models.get(i as usize).cloned())
}

pub fn refresh(ctx: &Rc<Ctx>) {
    load_models(ctx);
    let ui = ctx.ui();
    let d = ui.global::<VehiclesData>();
    match d.get_tab() {
        0 => {
            let status = match d.get_status_filter() {
                1 => Some(VehicleStatus::InStock),
                2 => Some(VehicleStatus::Booked),
                3 => Some(VehicleStatus::Demo),
                4 => Some(VehicleStatus::InTransit),
                5 => Some(VehicleStatus::Sold),
                _ => None,
            };
            let mut list = match ctx.pos.list_vehicles(&VehicleFilter {
                search: d.get_search().to_string(),
                status,
                ..Default::default()
            }) {
                Ok(l) => l,
                Err(e) => {
                    ctx.err(e);
                    return;
                }
            };
            if status.is_none() {
                list.retain(|v| v.status != VehicleStatus::Sold);
            }
            let bookings = ctx.pos.list_bookings(Some(BookingStatus::Open)).unwrap_or_default();
            let rows: Vec<BikeRow> = list
                .iter()
                .map(|v| {
                    let b = bookings.iter().find(|b| b.vehicle_id == Some(v.id));
                    let mut row = bike_row(ctx, v, &b.map(|b| format!("Booked {}", b.booking_no)).unwrap_or_default());
                    if let Some(b) = b {
                        row.customer = b.customer_name.clone().into();
                    }
                    row
                })
                .collect();
            d.set_rows(model(rows));
        }
        1 => {
            let status = match d.get_booking_filter() {
                0 => Some(BookingStatus::Open),
                1 => Some(BookingStatus::Delivered),
                2 => Some(BookingStatus::Cancelled),
                _ => None,
            };
            let list = ctx.pos.list_bookings(status).unwrap_or_default();
            d.set_bookings(model(
                list.iter()
                    .map(|b| BookingRow {
                        id: b.id as i32,
                        no: b.booking_no.clone().into(),
                        customer: b.customer_name.clone().into(),
                        phone: b.customer_phone.clone().into(),
                        bike: if b.chassis_no.is_empty() {
                            format!("{} · no bike chosen yet{}", b.model_name, if b.color.is_empty() { String::new() } else { format!(" · {}", b.color) })
                        } else {
                            format!("{} · {}", b.model_name, b.chassis_no)
                        }
                        .into(),
                        agreed: ctx.money(b.agreed_price),
                        advance: ctx.money(b.advance),
                        balance: ctx.money((b.agreed_price - b.advance).max(0)),
                        expected: if b.expected_date.is_empty() { "—".into() } else { pretty_date(&b.expected_date).into() },
                        created: pretty_date(&b.created_at).into(),
                        status: b.status.label().into(),
                        open: b.status == BookingStatus::Open,
                        has_bike: b.vehicle_id.is_some(),
                    })
                    .collect(),
            ));
        }
        _ => {}
    }
}

fn bike_form(ctx: &Ctx, v: Option<&Vehicle>) -> BikeForm {
    let fmt = ctx.pos.money();
    match v {
        Some(v) => BikeForm {
            id: v.id as i32,
            model: model_index(v.model_id),
            chassis: v.chassis_no.clone().into(),
            motor: v.motor_no.clone().into(),
            battery: v.battery_serial.clone().into(),
            color: v.color.clone().into(),
            year: if v.year > 0 { v.year.to_string().into() } else { SharedString::new() },
            cost: fmt.edit(v.cost).into(),
            price: fmt.edit(v.price).into(),
            status: match v.status {
                VehicleStatus::InTransit => 1,
                VehicleStatus::Demo => 2,
                _ => 0,
            },
            reg: v.registration_no.clone().into(),
            notes: v.notes.clone().into(),
        },
        None => BikeForm {
            model: -1,
            year: today()[..4].to_string().into(),
            ..Default::default()
        },
    }
}

/// Lines for a vehicle sale: the bike plus any accessories.
fn sell_lines(ctx: &Ctx, price: Money) -> Option<Vec<CartLine>> {
    let (vid, products) = ST.with(|st| {
        let st = st.borrow();
        (st.sell_vehicle, st.sell_products.clone())
    });
    let v = ctx.pos.get_vehicle(vid).ok()?;
    let mut lines = vec![CartLine {
        item_type: ItemType::Vehicle,
        ref_id: Some(v.id),
        code: v.chassis_no.clone(),
        description: format!("{}{}", v.model_name, if v.color.is_empty() { String::new() } else { format!(" — {}", v.color) }),
        qty: 1,
        unit_price: price,
        line_discount: 0,
        serial_no: v.chassis_no.clone(),
        free: false,
    }];
    for pid in products {
        if let Ok(p) = ctx.pos.get_product(pid) {
            lines.push(CartLine {
                item_type: ItemType::Product,
                ref_id: Some(p.id),
                code: p.code.clone(),
                description: p.name.clone(),
                qty: 1,
                unit_price: p.price,
                line_discount: 0,
                serial_no: String::new(),
                free: false,
            });
        }
    }
    Some(lines)
}

fn render_sell_lines(ctx: &Ctx) {
    let ids = ST.with(|st| st.borrow().sell_products.clone());
    let rows: Vec<SellLine> = ids
        .iter()
        .filter_map(|id| ctx.pos.get_product(*id).ok())
        .map(|p| SellLine { product_id: p.id as i32, code: p.code.into(), name: p.name.into(), price: ctx.money(p.price) })
        .collect();
    ctx.ui().global::<VehiclesData>().set_sell_lines(model(rows));
}

fn open_sell(ctx: &Rc<Ctx>, vehicle_id: i64) {
    let Ok(mut v) = ctx.pos.get_vehicle(vehicle_id) else { return };
    if matches!(v.status, VehicleStatus::Sold | VehicleStatus::External | VehicleStatus::InTransit) {
        ctx.err(format!("Bike {} can't be sold: it is {}.", v.chassis_no, v.status.label().to_lowercase()));
        return;
    }
    // A booking waiting for a bike: attach this bike to it.
    let pending = ST.with(|st| st.borrow_mut().pending_booking.take());
    if let Some(bid) = pending {
        if let Err(e) = ctx.pos.assign_booking_vehicle(bid, v.id) {
            ctx.err(e);
            return;
        }
        v = ctx.pos.get_vehicle(vehicle_id).unwrap_or(v);
    }
    let booking = if v.status == VehicleStatus::Booked {
        ctx.pos
            .list_bookings(Some(BookingStatus::Open))
            .unwrap_or_default()
            .into_iter()
            .find(|b| b.vehicle_id == Some(v.id))
    } else {
        None
    };
    let ui = ctx.ui();
    let d = ui.global::<VehiclesData>();
    let fmt = ctx.pos.money();
    d.set_sell_bike(bike_row(ctx, &v, ""));
    match &booking {
        Some(b) => {
            d.set_sell_customer_id(b.customer_id as i32);
            d.set_sell_customer_name(b.customer_name.clone().into());
            d.set_sell_customer_phone(b.customer_phone.clone().into());
            d.set_sell_booking(b.booking_no.clone().into());
            d.set_sell_advance(ctx.money(b.advance));
            d.set_sell_has_advance(b.advance > 0);
            d.set_sell_price(fmt.edit(if b.agreed_price > 0 { b.agreed_price } else { v.price }).into());
        }
        None => {
            d.set_sell_customer_id(0);
            d.set_sell_customer_name("".into());
            d.set_sell_customer_phone("".into());
            d.set_sell_booking("".into());
            d.set_sell_advance("".into());
            d.set_sell_has_advance(false);
            d.set_sell_price(fmt.edit(v.price).into());
        }
    }
    ST.with(|st| {
        let mut st = st.borrow_mut();
        st.sell_vehicle = v.id;
        st.sell_booking = booking.as_ref().map(|b| b.id);
        st.sell_advance = booking.as_ref().map(|b| b.advance).unwrap_or(0);
        st.sell_products.clear();
    });
    render_sell_lines(ctx);
    d.set_sell_error("".into());
    d.set_show_sell(true);
}

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let d = ui.global::<VehiclesData>();

    {
        let ctx = ctx.clone();
        d.on_refresh(move || refresh(&ctx));
    }
    {
        let ctx = ctx.clone();
        d.on_new_bike(move || {
            load_models(&ctx);
            let ui = ctx.ui();
            let d = ui.global::<VehiclesData>();
            d.set_bike_form(bike_form(&ctx, None));
            d.set_bike_error("".into());
            d.set_show_bike_form(true);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_edit_bike(move |id| {
            load_models(&ctx);
            if let Ok(v) = ctx.pos.get_vehicle(id as i64) {
                let ui = ctx.ui();
                let d = ui.global::<VehiclesData>();
                d.set_bike_form(bike_form(&ctx, Some(&v)));
                d.set_bike_error("".into());
                d.set_show_bike_form(true);
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_model_prices(move |i| {
            let fmt = ctx.pos.money();
            match model_at(i) {
                Some(m) => strings(vec![fmt.edit(m.price), fmt.edit(m.cost)]),
                None => strings(vec![String::new(), String::new()]),
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_save_bike(move |f| {
            let ui = ctx.ui();
            let d = ui.global::<VehiclesData>();
            let r = (|| -> Result<i64, String> {
                let m = model_at(f.model).ok_or("Please choose the model.")?;
                let existing = if f.id > 0 { ctx.pos.get_vehicle(f.id as i64).ok() } else { None };
                let cost = if ctx.pos.can(Perm::ViewCost) {
                    money_in(&f.cost, "Cost price")?
                } else {
                    existing.as_ref().map(|v| v.cost).unwrap_or(m.cost)
                };
                ctx.pos
                    .save_vehicle(VehicleInput {
                        id: if f.id > 0 { Some(f.id as i64) } else { None },
                        model_id: Some(m.id),
                        model_name: m.name.clone(),
                        chassis_no: f.chassis.to_string(),
                        motor_no: f.motor.to_string(),
                        battery_serial: f.battery.to_string(),
                        color: f.color.to_string(),
                        year: int_in(&f.year, "Year")?,
                        cost,
                        price: money_in(&f.price, "Sale price")?,
                        status: match f.status {
                            1 => VehicleStatus::InTransit,
                            2 => VehicleStatus::Demo,
                            _ => VehicleStatus::InStock,
                        },
                        registration_no: f.reg.to_string(),
                        notes: f.notes.to_string(),
                        customer_id: None,
                    })
                    .map_err(|e| e.to_string())
            })();
            match r {
                Ok(_) => {
                    d.set_show_bike_form(false);
                    ctx.ok(if f.id > 0 { "Bike updated" } else { "Bike added to stock" });
                    refresh(&ctx);
                    true
                }
                Err(e) => {
                    d.set_bike_error(e.into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_new_model(move || {
            let ui = ctx.ui();
            let d = ui.global::<VehiclesData>();
            d.set_model_form(ModelForm {
                vw: "12".into(),
                bw: "24".into(),
                mw: "24".into(),
                free: "3".into(),
                active: true,
                ..Default::default()
            });
            d.set_model_error("".into());
            d.set_show_model_form(true);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_edit_model(move |id| {
            if let Ok(m) = ctx.pos.get_vehicle_model(id as i64) {
                let fmt = ctx.pos.money();
                let ui = ctx.ui();
                let d = ui.global::<VehiclesData>();
                d.set_model_form(ModelForm {
                    id: m.id as i32,
                    name: m.name.into(),
                    brand: m.brand.into(),
                    price: fmt.edit(m.price).into(),
                    cost: fmt.edit(m.cost).into(),
                    vw: m.vehicle_warranty_months.to_string().into(),
                    bw: m.battery_warranty_months.to_string().into(),
                    mw: m.motor_warranty_months.to_string().into(),
                    km: m.warranty_km.to_string().into(),
                    free: m.free_services.to_string().into(),
                    active: m.active,
                });
                d.set_model_error("".into());
                d.set_show_model_form(true);
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_save_model(move |f| {
            let ui = ctx.ui();
            let d = ui.global::<VehiclesData>();
            let r = (|| -> Result<i64, String> {
                let existing = if f.id > 0 { ctx.pos.get_vehicle_model(f.id as i64).ok() } else { None };
                let cost = if ctx.pos.can(Perm::ViewCost) {
                    money_in(&f.cost, "Cost price")?
                } else {
                    existing.map(|m| m.cost).unwrap_or(0)
                };
                ctx.pos
                    .save_vehicle_model(VehicleModel {
                        id: f.id as i64,
                        name: f.name.to_string(),
                        brand: f.brand.to_string(),
                        price: money_in(&f.price, "Sale price")?,
                        cost,
                        vehicle_warranty_months: int_in(&f.vw, "Vehicle warranty")?,
                        battery_warranty_months: int_in(&f.bw, "Battery warranty")?,
                        motor_warranty_months: int_in(&f.mw, "Motor warranty")?,
                        warranty_km: int_in(&f.km, "Warranty km")?,
                        free_services: int_in(&f.free, "Free services")?,
                        active: f.active,
                        in_stock: 0,
                    })
                    .map_err(|e| e.to_string())
            })();
            match r {
                Ok(_) => {
                    d.set_show_model_form(false);
                    ctx.ok("Model saved");
                    refresh(&ctx);
                    true
                }
                Err(e) => {
                    d.set_model_error(e.into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_open_sell(move |id| open_sell(&ctx, id as i64));
    }
    {
        let ctx = ctx.clone();
        d.on_sell_add(move |id| {
            ST.with(|st| st.borrow_mut().sell_products.push(id as i64));
            render_sell_lines(&ctx);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_sell_remove(move |i| {
            ST.with(|st| {
                let mut st = st.borrow_mut();
                if (i as usize) < st.sell_products.len() {
                    st.sell_products.remove(i as usize);
                }
            });
            render_sell_lines(&ctx);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_sell_summary(move |f| {
            let price = parse_money(&f.price).unwrap_or(0);
            let discount = parse_money(&f.discount).unwrap_or(0);
            let cash = parse_money(&f.cash).unwrap_or(0);
            let bank = parse_money(&f.bank).unwrap_or(0);
            let advance = ST.with(|st| st.borrow().sell_advance);
            let lines = sell_lines(&ctx, price).unwrap_or_default();
            let t = ctx.pos.totals(&lines, discount);
            let paying = cash + bank;
            let balance = t.total - advance - paying;
            let mut v = vec![InfoPair { label: "Bike".into(), value: ctx.money(price), tone: 0 }];
            let acc: Money = lines.iter().skip(1).map(|l| l.gross()).sum();
            if acc > 0 {
                v.push(InfoPair { label: "Accessories".into(), value: ctx.money(acc), tone: 0 });
            }
            if t.discount > 0 {
                v.push(InfoPair { label: "Discount".into(), value: format!("− {}", ctx.money(t.discount)).into(), tone: 2 });
            }
            if t.tax > 0 {
                v.push(InfoPair { label: "Tax".into(), value: ctx.money(t.tax), tone: 0 });
            }
            v.push(InfoPair { label: "Total".into(), value: ctx.money(t.total), tone: 9 });
            if advance > 0 {
                v.push(InfoPair { label: "Advance paid".into(), value: format!("− {}", ctx.money(advance)).into(), tone: 2 });
            }
            v.push(InfoPair { label: "Paying now".into(), value: ctx.money(paying), tone: 0 });
            v.push(InfoPair {
                label: if balance > 0 { "Balance due".into() } else if balance < 0 { "Over-paid".into() } else { "Balance".into() },
                value: ctx.money(balance.abs()),
                tone: if balance != 0 { 1 } else { 2 },
            });
            model(v)
        });
    }
    {
        let ctx = ctx.clone();
        d.on_complete_sale(move |f| complete_sale(&ctx, &f));
    }
    {
        let ctx = ctx.clone();
        d.on_open_book(move |id| {
            let Ok(v) = ctx.pos.get_vehicle(id as i64) else { return };
            let ui = ctx.ui();
            let d = ui.global::<VehiclesData>();
            d.set_book_bike(bike_row(&ctx, &v, ""));
            d.set_book_has_bike(true);
            d.set_book_price(ctx.pos.money().edit(v.price).into());
            d.set_book_error("".into());
            ST.with(|st| st.borrow_mut().book_vehicle = Some(v.id));
            d.set_show_book(true);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_open_book_model(move || {
            load_models(&ctx);
            let ui = ctx.ui();
            let d = ui.global::<VehiclesData>();
            d.set_book_bike(BikeRow::default());
            d.set_book_has_bike(false);
            d.set_book_price("".into());
            d.set_book_error("".into());
            ST.with(|st| st.borrow_mut().book_vehicle = None);
            d.set_show_book(true);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_save_booking(move |f| {
            let ui = ctx.ui();
            let d = ui.global::<VehiclesData>();
            let r = (|| -> Result<String, String> {
                if f.customer_id <= 0 {
                    return Err("Please choose the customer.".into());
                }
                let vehicle = ST.with(|st| st.borrow().book_vehicle);
                let model_id = match vehicle {
                    Some(_) => None,
                    None => Some(model_at(f.model).ok_or("Please choose the model.")?.id),
                };
                let expected = f.expected.trim().to_string();
                if !expected.is_empty() && pos_core::util::parse_date(&expected).is_none() {
                    return Err("Expected delivery must look like 2026-12-31.".into());
                }
                let cash = money_in(&f.cash, "Cash advance")?;
                let bank = money_in(&f.bank, "Bank advance")?;
                let mut advance = Vec::new();
                if cash > 0 {
                    advance.push(PaymentInput::cash(cash));
                }
                if bank > 0 {
                    advance.push(PaymentInput::bank(bank, &f.bank_name, &f.reference));
                }
                ctx.pos
                    .create_booking(BookingInput {
                        customer_id: f.customer_id as i64,
                        model_id,
                        vehicle_id: vehicle,
                        color: f.color.to_string(),
                        agreed_price: money_in(&f.agreed, "Agreed price")?,
                        expected_date: expected,
                        notes: f.notes.to_string(),
                        advance,
                    })
                    .map_err(|e| e.to_string())
            })();
            match r {
                Ok(no) => {
                    d.set_show_book(false);
                    ctx.ok(format!("Booking {no} saved"));
                    refresh(&ctx);
                    true
                }
                Err(e) => {
                    d.set_book_error(e.into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_open_booking_pay(move |id| {
            if let Ok(b) = ctx.pos.get_booking(id as i64) {
                let ui = ctx.ui();
                let d = ui.global::<VehiclesData>();
                d.set_booking_pay_title(
                    format!("{} · {} · paid {} of {}", b.booking_no, b.customer_name, ctx.money(b.advance), ctx.money(b.agreed_price)).into(),
                );
                d.set_booking_pay_error("".into());
                ST.with(|st| st.borrow_mut().pay_booking = b.id);
                d.set_show_booking_pay(true);
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_save_booking_pay(move |method, amount, reference, _| {
            let ui = ctx.ui();
            let d = ui.global::<VehiclesData>();
            let id = ST.with(|st| st.borrow().pay_booking);
            let amt = match money_in(&amount, "Amount") {
                Ok(a) => a,
                Err(e) => {
                    d.set_booking_pay_error(e.into());
                    return false;
                }
            };
            let p = if method == 1 { PaymentInput::bank(amt, &reference, &reference) } else { PaymentInput::cash(amt) };
            match ctx.pos.add_booking_payment(id, p) {
                Ok(()) => {
                    d.set_show_booking_pay(false);
                    ctx.ok("Payment added to booking");
                    refresh(&ctx);
                    true
                }
                Err(e) => {
                    d.set_booking_pay_error(e.to_string().into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_open_cancel(move |id| {
            if let Ok(b) = ctx.pos.get_booking(id as i64) {
                let ui = ctx.ui();
                let d = ui.global::<VehiclesData>();
                d.set_cancel_title(format!("{} · {}", b.booking_no, b.customer_name).into());
                d.set_cancel_advance(ctx.money(b.advance));
                ST.with(|st| st.borrow_mut().cancel_booking = b.id);
                d.set_show_cancel(true);
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_confirm_cancel(move |refund| {
            let id = ST.with(|st| st.borrow().cancel_booking);
            let method = match refund {
                0 => Some(PayMethod::Cash),
                1 => Some(PayMethod::Bank),
                _ => None,
            };
            match ctx.pos.cancel_booking(id, method) {
                Ok(()) => {
                    ctx.ui().global::<VehiclesData>().set_show_cancel(false);
                    ctx.ok("Booking cancelled");
                    refresh(&ctx);
                }
                Err(e) => ctx.err(e),
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_deliver_booking(move |id| {
            let Ok(b) = ctx.pos.get_booking(id as i64) else { return };
            match b.vehicle_id {
                Some(vid) => open_sell(&ctx, vid),
                None => {
                    ST.with(|st| st.borrow_mut().pending_booking = Some(b.id));
                    let ui = ctx.ui();
                    let d = ui.global::<VehiclesData>();
                    d.set_tab(0);
                    d.set_status_filter(1);
                    d.set_search(b.model_name.clone().into());
                    refresh(&ctx);
                    ctx.ok(format!("Pick a {} for {} and click Sell", b.model_name, b.customer_name));
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_open_detail(move |id| open_detail(&ctx, id as i64));
    }
    {
        let ctx = ctx.clone();
        d.on_print_invoice(move || {
            let id = ST.with(|st| st.borrow().last_sale);
            if id > 0 {
                ctx.print_sale(id, crate::print::Paper::A4);
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_save_reg(move |reg| {
            let id = ST.with(|st| st.borrow().detail_vehicle);
            match ctx.pos.set_registration(id, &reg) {
                Ok(()) => {
                    ctx.ok("Registration saved");
                    refresh(&ctx);
                }
                Err(e) => ctx.err(e),
            }
        });
    }
}

fn complete_sale(ctx: &Rc<Ctx>, f: &SellForm) -> bool {
    let ui = ctx.ui();
    let d = ui.global::<VehiclesData>();
    let r = (|| -> Result<pos_core::SaleReceipt, String> {
        if f.customer_id <= 0 {
            return Err("Please choose the customer buying the bike.".into());
        }
        let price = money_in(&f.price, "Bike price")?;
        let discount = money_in(&f.discount, "Discount")?;
        let cash = money_in(&f.cash, "Cash")?;
        let bank = money_in(&f.bank, "Bank transfer")?;
        let lines = sell_lines(ctx, price).ok_or("Bike not found.")?;
        let (booking, vid) = ST.with(|st| {
            let st = st.borrow();
            (st.sell_booking, st.sell_vehicle)
        });
        let mut payments = Vec::new();
        if cash > 0 {
            payments.push(PaymentInput::cash(cash));
        }
        if bank > 0 {
            payments.push(PaymentInput::bank(bank, &f.bank_name, &f.reference));
        }
        let r = ctx
            .pos
            .checkout(CheckoutInput {
                kind: SaleKind::Vehicle,
                customer_id: Some(f.customer_id as i64),
                salesperson_id: None,
                items: lines,
                discount,
                payments,
                notes: f.notes.to_string(),
                booking_id: booking,
            })
            .map_err(|e| e.to_string())?;
        if !f.reg.trim().is_empty() {
            let _ = ctx.pos.set_registration(vid, &f.reg);
        }
        Ok(r)
    })();
    match r {
        Ok(r) => {
            let vid = ST.with(|st| st.borrow().sell_vehicle);
            let mut w = Vec::new();
            if let Ok(v) = ctx.pos.get_vehicle(vid) {
                let mut add = |label: &str, until: &str| {
                    if !until.is_empty() {
                        w.push(InfoPair { label: label.into(), value: format!("until {}", pretty_date(until)).into(), tone: 0 });
                    }
                };
                add("Vehicle warranty", &v.vehicle_warranty_until);
                add("Battery warranty", &v.battery_warranty_until);
                add("Motor warranty", &v.motor_warranty_until);
                if v.coupons_left > 0 {
                    w.push(InfoPair { label: "Free service coupons".into(), value: format!("{} coupons", v.coupons_left).into(), tone: 0 });
                }
            }
            d.set_done_warranty(model(w));
            ST.with(|st| st.borrow_mut().last_sale = r.sale_id);
            d.set_done_invoice(r.invoice_no.into());
            d.set_done_total(ctx.money(r.total));
            d.set_done_balance(ctx.money(r.balance));
            d.set_done_has_balance(r.balance > 0);
            d.set_show_sell(false);
            d.set_show_sell_done(true);
            refresh(ctx);
            ctx.update_badges();
            true
        }
        Err(e) => {
            d.set_sell_error(e.into());
            false
        }
    }
}

fn open_detail(ctx: &Rc<Ctx>, id: i64) {
    let Ok(v) = ctx.pos.get_vehicle(id) else { return };
    ST.with(|st| st.borrow_mut().detail_vehicle = v.id);
    let ui = ctx.ui();
    let d = ui.global::<VehiclesData>();
    let p = |l: &str, val: String| InfoPair { label: l.into(), value: val.into(), tone: 0 };
    let mut info = vec![
        p("CHASSIS NO.", v.chassis_no.clone()),
        p("MOTOR NO.", v.motor_no.clone()),
        p("BATTERY SERIAL", v.battery_serial.clone()),
        p("COLOUR", v.color.clone()),
        p("YEAR", if v.year > 0 { v.year.to_string() } else { String::new() }),
        p("STATUS", v.status.label().into()),
        p("PRICE", ctx.money(v.price).to_string()),
    ];
    if ctx.pos.can(Perm::ViewCost) {
        info.push(p("COST", ctx.money(v.cost).to_string()));
    }
    info.push(p("RECEIVED", pretty_date(&v.received_at)));
    if v.customer_id.is_some() {
        info.push(p("OWNER", v.customer_name.clone()));
        info.push(p("PHONE", v.customer_phone.clone()));
    }
    if !v.sold_at.is_empty() {
        info.push(p("SOLD ON", pretty_date(&v.sold_at)));
        info.push(p("INVOICE", v.invoice_no.clone()));
    }
    d.set_detail_info(model(info));
    let t = today();
    let mut w = Vec::new();
    for (label, until) in [
        ("Vehicle warranty", &v.vehicle_warranty_until),
        ("Battery warranty", &v.battery_warranty_until),
        ("Motor warranty", &v.motor_warranty_until),
    ] {
        if !until.is_empty() {
            let active = until.as_str() >= t.as_str();
            w.push(InfoPair {
                label: label.into(),
                value: if active { format!("Active until {}", pretty_date(until)) } else { format!("Expired {}", pretty_date(until)) }.into(),
                tone: if active { 0 } else { 1 },
            });
        }
    }
    let coupons = ctx.pos.vehicle_coupons(v.id).unwrap_or_default();
    if !coupons.is_empty() {
        let used = coupons.iter().filter(|c| c.used).count();
        w.push(InfoPair {
            label: "Free services".into(),
            value: format!("{} of {} left", coupons.len() - used, coupons.len()).into(),
            tone: 0,
        });
    }
    d.set_detail_warranty(model(w));
    let jobs = ctx.pos.vehicle_history(v.id).unwrap_or_default();
    d.set_detail_history(model(
        jobs.iter()
            .map(|j| HistRow {
                title: format!("{} · {}", j.job_no, j.complaint).into(),
                sub: format!(
                    "{}{}{}",
                    pretty_date(&j.created_at),
                    if j.odometer > 0 { format!(" · {} km", j.odometer) } else { String::new() },
                    if j.technician_name.is_empty() { String::new() } else { format!(" · {}", j.technician_name) }
                )
                .into(),
                right: ctx.money(j.total()),
                badge: j.status.label().into(),
            })
            .collect(),
    ));
    d.set_detail_title(format!("{}{}", v.model_name, if v.color.is_empty() { String::new() } else { format!(" · {}", v.color) }).into());
    d.set_detail_sub(
        if v.registration_no.is_empty() { format!("Chassis {}", v.chassis_no) } else { format!("{} · Chassis {}", v.registration_no, v.chassis_no) }
            .into(),
    );
    d.set_detail_reg(v.registration_no.clone().into());
    d.set_detail_sold(v.status == VehicleStatus::Sold);
    d.set_show_detail(true);
}
