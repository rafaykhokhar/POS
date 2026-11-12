use super::*;
use crate::models::*;
use crate::util::today;

const RS: i64 = 100;

fn shop(tax_bp: i64, inclusive: bool) -> Pos {
    let pos = Pos::open_in_memory().unwrap();
    let settings = Settings {
        shop_name: "Test Motors".into(),
        tax_rate_bp: tax_bp,
        tax_inclusive: inclusive,
        ..Default::default()
    };
    pos.complete_setup(SetupInput {
        settings,
        owner_name: "Owner".into(),
        owner_pin: "1234".into(),
        demo_data: false,
    })
    .unwrap();
    pos
}

fn product(pos: &Pos, code: &str, cost: i64, price: i64, stock: i64) -> i64 {
    pos.save_product(ProductInput {
        code: code.into(),
        name: format!("Item {code}"),
        cost: cost * RS,
        price: price * RS,
        stock,
        low_stock: 3,
        ..Default::default()
    })
    .unwrap()
}

fn line(pos: &Pos, id: i64, qty: i64) -> CartLine {
    let p = pos.get_product(id).unwrap();
    CartLine {
        item_type: ItemType::Product,
        ref_id: Some(id),
        code: p.code,
        description: p.name,
        qty,
        unit_price: p.price,
        line_discount: 0,
        serial_no: String::new(),
        free: false,
    }
}

fn counter(items: Vec<CartLine>, discount: i64, pay: Vec<PaymentInput>, customer: Option<i64>) -> CheckoutInput {
    CheckoutInput {
        kind: SaleKind::Counter,
        customer_id: customer,
        salesperson_id: None,
        items,
        discount,
        payments: pay,
        notes: String::new(),
        booking_id: None,
    }
}

#[test]
fn setup_and_login() {
    let pos = shop(0, true);
    assert!(pos.is_setup_done());
    let owner = pos.current_user().unwrap();
    pos.logout();
    assert!(pos.current_user().is_none());
    assert!(pos.login(owner.id, "0000").is_err());
    assert!(pos.login(owner.id, "1234").is_ok());
}

#[test]
fn codes_are_unique_and_suggested() {
    let pos = shop(0, true);
    let cat = pos.save_category(None, "Brakes", "BRK").unwrap();
    assert_eq!(pos.suggest_code(Some(cat)).unwrap(), "BRK-001");
    pos.save_product(ProductInput {
        code: "brk-001".into(),
        name: "Pads".into(),
        category_id: Some(cat),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(pos.get_product(1).unwrap().code, "BRK-001");
    assert_eq!(pos.suggest_code(Some(cat)).unwrap(), "BRK-002");
    let dup = pos.save_product(ProductInput { code: "BRK-001".into(), name: "Other".into(), ..Default::default() });
    assert!(dup.is_err());
    assert!(pos.find_by_code("brk-001").unwrap().is_some());
}

#[test]
fn stock_levels() {
    assert_eq!(stock_level(0, 3), StockLevel::Out);
    assert_eq!(stock_level(3, 3), StockLevel::Low);
    assert_eq!(stock_level(1, 3), StockLevel::Low);
    assert_eq!(stock_level(5, 3), StockLevel::Warn);
    assert_eq!(stock_level(7, 3), StockLevel::Ok);
}

#[test]
fn counter_sale_updates_stock_and_profit() {
    let pos = shop(0, true);
    let a = product(&pos, "A1", 600, 1000, 10);
    let b = product(&pos, "B1", 300, 500, 5);
    let items = vec![line(&pos, a, 2), line(&pos, b, 1)];
    let r = pos.checkout(counter(items, 500 * RS, vec![PaymentInput::cash(2000 * RS)], None)).unwrap();
    assert_eq!(r.total, 2000 * RS);
    assert_eq!(pos.get_product(a).unwrap().stock, 8);
    assert_eq!(pos.get_product(b).unwrap().stock, 4);
    let t = today();
    let sheet = pos.profit_sheet(&t, &t).unwrap();
    assert_eq!(sheet.revenue, 2000 * RS);
    assert_eq!(sheet.cost, 1500 * RS);
    assert_eq!(sheet.profit, 500 * RS);
    // Per-item revenue adds up exactly to the bill (discount spread across lines).
    let sum: i64 = sheet.items.iter().map(|i| i.revenue).sum();
    assert_eq!(sum, 2000 * RS);
}

#[test]
fn cannot_oversell() {
    let pos = shop(0, true);
    let a = product(&pos, "A1", 100, 200, 2);
    let err = pos.checkout(counter(vec![line(&pos, a, 3)], 0, vec![PaymentInput::cash(600 * RS)], None));
    assert!(err.is_err());
    assert_eq!(pos.get_product(a).unwrap().stock, 2);
}

#[test]
fn unpaid_needs_customer() {
    let pos = shop(0, true);
    let a = product(&pos, "A1", 100, 200, 5);
    assert!(pos.checkout(counter(vec![line(&pos, a, 1)], 0, vec![], None)).is_err());
    let c = pos.save_customer(CustomerInput { name: "Ali".into(), ..Default::default() }).unwrap();
    let r = pos.checkout(counter(vec![line(&pos, a, 2)], 0, vec![PaymentInput::cash(100 * RS)], Some(c))).unwrap();
    assert_eq!(r.balance, 300 * RS);
    assert_eq!(pos.get_customer(c).unwrap().balance, 300 * RS);
    pos.receive_customer_payment(c, PaymentInput::bank(300 * RS, "HBL", "X1")).unwrap();
    assert_eq!(pos.get_customer(c).unwrap().balance, 0);
    assert!(pos.receive_customer_payment(c, PaymentInput::cash(1)).is_err());
}

#[test]
fn tax_exclusive_and_inclusive() {
    let lines = vec![CartLine {
        item_type: ItemType::Custom,
        ref_id: None,
        code: String::new(),
        description: "x".into(),
        qty: 1,
        unit_price: 10_000,
        line_discount: 0,
        serial_no: String::new(),
        free: false,
    }];
    let ex = compute_totals(&lines, 0, 1700, false);
    assert_eq!((ex.tax, ex.total), (1_700, 11_700));
    let inc = compute_totals(&lines, 0, 1700, true);
    assert_eq!(inc.total, 10_000);
    assert_eq!(inc.tax, 1_453);
}

#[test]
fn profit_excludes_tax() {
    let pos = shop(1700, false);
    let a = product(&pos, "A1", 600, 1000, 10);
    let total = pos.totals(&[line(&pos, a, 1)], 0).total;
    assert_eq!(total, 1170 * RS);
    pos.checkout(counter(vec![line(&pos, a, 1)], 0, vec![PaymentInput::cash(total)], None)).unwrap();
    let t = today();
    let sheet = pos.profit_sheet(&t, &t).unwrap();
    assert_eq!(sheet.revenue, 1000 * RS);
    assert_eq!(sheet.tax, 170 * RS);
    assert_eq!(sheet.profit, 400 * RS);
}

#[test]
fn receiving_stock_averages_cost() {
    let pos = shop(0, true);
    let a = product(&pos, "A1", 100, 200, 10);
    pos.receive_stock(ReceiveInput {
        supplier_id: None,
        items: vec![ReceiveLine { product_id: a, qty: 10, unit_cost: 200 * RS }],
        notes: String::new(),
    })
    .unwrap();
    let p = pos.get_product(a).unwrap();
    assert_eq!(p.stock, 20);
    assert_eq!(p.cost, 150 * RS);
}

#[test]
fn returns_restock_and_refund() {
    let pos = shop(0, true);
    let a = product(&pos, "A1", 600, 1000, 10);
    let r = pos.checkout(counter(vec![line(&pos, a, 3)], 0, vec![PaymentInput::cash(3000 * RS)], None)).unwrap();
    let sale = pos.get_sale(r.sale_id).unwrap();
    let item = sale.items[0].id;
    let ret = pos.return_items(r.sale_id, &[(item, 1)], Some(PayMethod::Cash), "Wrong size").unwrap();
    assert_eq!(ret.total, -1000 * RS);
    assert_eq!(pos.get_product(a).unwrap().stock, 8);
    assert!(pos.return_items(r.sale_id, &[(item, 3)], Some(PayMethod::Cash), "").is_err());
    let t = today();
    let sheet = pos.profit_sheet(&t, &t).unwrap();
    assert_eq!(sheet.revenue, 2000 * RS);
    assert_eq!(sheet.profit, 800 * RS);
}

fn bike_setup(pos: &Pos) -> (i64, i64, i64) {
    let m = pos
        .save_vehicle_model(VehicleModel {
            name: "Spark S1".into(),
            price: 285_000 * RS,
            cost: 240_000 * RS,
            vehicle_warranty_months: 12,
            battery_warranty_months: 24,
            free_services: 3,
            active: true,
            ..Default::default()
        })
        .unwrap();
    let v = pos
        .save_vehicle(VehicleInput {
            model_id: Some(m),
            chassis_no: "ch-001".into(),
            cost: 240_000 * RS,
            price: 285_000 * RS,
            ..Default::default()
        })
        .unwrap();
    let c = pos.save_customer(CustomerInput { name: "Sara".into(), ..Default::default() }).unwrap();
    (m, v, c)
}

fn bike_line(pos: &Pos, v: i64) -> CartLine {
    let b = pos.get_vehicle(v).unwrap();
    CartLine {
        item_type: ItemType::Vehicle,
        ref_id: Some(v),
        code: b.chassis_no,
        description: b.model_name,
        qty: 1,
        unit_price: b.price,
        line_discount: 0,
        serial_no: String::new(),
        free: false,
    }
}

#[test]
fn vehicle_sale_sets_warranty_and_coupons() {
    let pos = shop(0, true);
    let (_, v, c) = bike_setup(&pos);
    assert_eq!(pos.get_vehicle(v).unwrap().chassis_no, "CH-001");
    let r = pos
        .checkout(CheckoutInput {
            kind: SaleKind::Vehicle,
            customer_id: Some(c),
            salesperson_id: None,
            items: vec![bike_line(&pos, v)],
            discount: 0,
            payments: vec![PaymentInput::bank(285_000 * RS, "HBL", "T1")],
            notes: String::new(),
            booking_id: None,
        })
        .unwrap();
    assert_eq!(r.balance, 0);
    let bike = pos.get_vehicle(v).unwrap();
    assert_eq!(bike.status, VehicleStatus::Sold);
    assert_eq!(bike.customer_id, Some(c));
    assert_eq!(bike.coupons_left, 3);
    assert!(!bike.vehicle_warranty_until.is_empty());
    assert!(bike.battery_warranty_until > bike.vehicle_warranty_until);
    let t = today();
    assert_eq!(pos.profit_sheet(&t, &t).unwrap().profit, 45_000 * RS);
    // Can't sell twice
    assert!(pos
        .checkout(CheckoutInput {
            kind: SaleKind::Vehicle,
            customer_id: Some(c),
            salesperson_id: None,
            items: vec![bike_line(&pos, v)],
            discount: 0,
            payments: vec![],
            notes: String::new(),
            booking_id: None,
        })
        .is_err());
}

#[test]
fn booking_advance_counts_at_delivery() {
    let pos = shop(0, true);
    let (m, v, c) = bike_setup(&pos);
    pos.create_booking(BookingInput {
        customer_id: c,
        model_id: Some(m),
        vehicle_id: Some(v),
        agreed_price: 285_000 * RS,
        advance: vec![PaymentInput::cash(50_000 * RS)],
        ..Default::default()
    })
    .unwrap();
    assert_eq!(pos.get_vehicle(v).unwrap().status, VehicleStatus::Booked);
    let b = pos.list_bookings(Some(BookingStatus::Open)).unwrap()[0].clone();
    assert_eq!(b.advance, 50_000 * RS);
    let r = pos
        .checkout(CheckoutInput {
            kind: SaleKind::Vehicle,
            customer_id: Some(c),
            salesperson_id: None,
            items: vec![bike_line(&pos, v)],
            discount: 0,
            payments: vec![PaymentInput::cash(235_000 * RS)],
            notes: String::new(),
            booking_id: Some(b.id),
        })
        .unwrap();
    assert_eq!(r.paid, 285_000 * RS);
    assert_eq!(r.balance, 0);
    assert_eq!(pos.get_booking(b.id).unwrap().status, BookingStatus::Delivered);
}

#[test]
fn workshop_job_flow() {
    let pos = shop(0, true);
    let (_, v, c) = bike_setup(&pos);
    pos.checkout(CheckoutInput {
        kind: SaleKind::Vehicle,
        customer_id: Some(c),
        salesperson_id: None,
        items: vec![bike_line(&pos, v)],
        discount: 0,
        payments: vec![PaymentInput::cash(285_000 * RS)],
        notes: String::new(),
        booking_id: None,
    })
    .unwrap();
    let pads = product(&pos, "BRK-001", 900, 1600, 10);
    let svc = pos
        .save_service(LaborService { id: 0, code: "SRV-01".into(), name: "Service".into(), price: 1500 * RS, active: true })
        .unwrap();
    let j = pos
        .create_job(JobInput {
            vehicle_id: v,
            complaint: "First service".into(),
            use_coupon: true,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(pos.get_vehicle(v).unwrap().coupons_left, 2);
    pos.add_job_service(j, Some(svc), "", 0, false).unwrap();
    pos.add_job_part(j, pads, 2, false).unwrap();
    assert_eq!(pos.get_product(pads).unwrap().stock, 8);
    let job = pos.get_job(j).unwrap();
    assert_eq!(job.labor_total, 0, "coupon makes the service free");
    assert_eq!(job.parts_total, 3200 * RS);
    // Second job on same bike is blocked while open
    assert!(pos.create_job(JobInput { vehicle_id: v, complaint: "x".into(), ..Default::default() }).is_err());
    let r = pos.deliver_job(j, 0, vec![PaymentInput::cash(3200 * RS)], "", 0).unwrap();
    assert_eq!(r.total, 3200 * RS);
    // Stock was not taken twice
    assert_eq!(pos.get_product(pads).unwrap().stock, 8);
    let job = pos.get_job(j).unwrap();
    assert_eq!(job.status, JobStatus::Delivered);
    assert!(!job.next_service_date.is_empty());
    let t = today();
    let sheet = pos.profit_sheet(&t, &t).unwrap();
    let wp = sheet.sections.iter().find(|s| s.section == Section::WorkshopParts).unwrap();
    assert_eq!(wp.profit, 1400 * RS);
}

#[test]
fn removing_job_part_restores_stock() {
    let pos = shop(0, true);
    let (_, v, _) = bike_setup(&pos);
    let pads = product(&pos, "BRK-001", 900, 1600, 5);
    // Treat the stock bike as a demo bike in the workshop
    let c = pos.save_customer(CustomerInput { name: "Walk-in".into(), ..Default::default() }).unwrap();
    let ext = pos.add_customer_bike(c, "Other EV", "", "ABC-1", "Red").unwrap();
    let _ = v;
    let j = pos.create_job(JobInput { vehicle_id: ext, complaint: "Brakes".into(), ..Default::default() }).unwrap();
    pos.add_job_part(j, pads, 2, false).unwrap();
    let item = pos.get_job(j).unwrap().items[0].id;
    pos.remove_job_item(item).unwrap();
    assert_eq!(pos.get_product(pads).unwrap().stock, 5);
    pos.add_job_part(j, pads, 1, false).unwrap();
    pos.set_job_status(j, JobStatus::Cancelled).unwrap();
    assert_eq!(pos.get_product(pads).unwrap().stock, 5);
}

#[test]
fn permissions_are_enforced() {
    let pos = shop(0, true);
    let a = product(&pos, "A1", 100, 200, 5);
    let cashier = pos
        .save_user(UserInput {
            name: "Hina".into(),
            role: Some(Role::Cashier),
            pin: Some("4444".into()),
            active: true,
            ..Default::default()
        })
        .unwrap();
    pos.login(cashier, "4444").unwrap();
    assert!(pos.save_product(ProductInput { code: "Z".into(), name: "Z".into(), ..Default::default() }).is_err());
    assert!(pos.profit_sheet(&today(), &today()).is_err());
    // Cashier can't discount
    let err = pos.checkout(counter(vec![line(&pos, a, 1)], 50 * RS, vec![PaymentInput::cash(150 * RS)], None));
    assert!(err.is_err());
    assert!(pos.checkout(counter(vec![line(&pos, a, 1)], 0, vec![PaymentInput::cash(200 * RS)], None)).is_ok());
}

#[test]
fn cash_drawer_expected_amount() {
    let pos = shop(0, true);
    let a = product(&pos, "A1", 100, 200, 10);
    pos.open_shift(5000 * RS).unwrap();
    pos.checkout(counter(vec![line(&pos, a, 2)], 0, vec![PaymentInput::cash(400 * RS)], None)).unwrap();
    pos.checkout(counter(vec![line(&pos, a, 1)], 0, vec![PaymentInput::bank(200 * RS, "HBL", "X")], None))
        .unwrap();
    pos.cash_move(-100 * RS, "Tea").unwrap();
    let s = pos.current_shift().unwrap().unwrap();
    assert_eq!(s.expected, 5300 * RS);
    let closed = pos.close_shift(5250 * RS, "").unwrap();
    assert_eq!(closed.difference(), Some(-50 * RS));
    assert!(pos.current_shift().unwrap().is_none());
}

#[test]
fn demo_data_loads_with_any_tax_setting() {
    for (bp, inc) in [(0, true), (1700, false), (1700, true)] {
        let pos = shop(bp, inc);
        pos.load_demo_data().unwrap();
        let d = pos.dashboard().unwrap();
        assert!(d.sales_month > 0);
        assert!(d.open_jobs >= 4);
        assert!(d.low_stock_count + d.out_of_stock_count > 0);
        assert!(!d.service_due.is_empty());
        let t = today();
        let sheet = pos.profit_sheet(&crate::util::add_days(&t, -30), &t).unwrap();
        assert!(sheet.profit > 0);
        assert!(pos.list_customers("", false, 100).unwrap().len() >= 6);
    }
}
