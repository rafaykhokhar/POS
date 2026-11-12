//! End-to-end test: drives the real app window (headless) through a full shop day
//! by calling the same UI callbacks the buttons call, then checks what the screens show.

use crate::ctx::Ctx;
use crate::screens::navigate;
use crate::*;
use pos_core::Pos;
use slint::{ComponentHandle, Model, ModelRc, SharedString};
use std::rc::Rc;

fn all<T: Clone + 'static>(m: ModelRc<T>) -> Vec<T> {
    m.iter().collect()
}

fn s(v: &str) -> SharedString {
    v.into()
}

fn start() -> (AppWindow, Rc<Ctx>, std::path::PathBuf) {
    i_slint_backend_testing::init_no_event_loop();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("evpos-e2e-{}-{nanos}", std::process::id()));
    let pos = Pos::open(&dir.join("pos.db")).unwrap();
    let ui = AppWindow::new().unwrap();
    let ctx = Rc::new(Ctx::new(pos, &ui, dir.clone()));
    crate::screens::init(&ctx);
    ctx.start();
    (ui, ctx, dir)
}

fn product(ui: &AppWindow, code: &str) -> ProductRow {
    let p = ui.global::<ProductsData>();
    p.set_search(s(""));
    p.set_category_filter(0);
    p.set_level_filter(0);
    p.invoke_refresh();
    all(p.get_rows())
        .into_iter()
        .find(|r| r.code == code)
        .unwrap_or_else(|| panic!("product {code} not in the Parts & Stock list"))
}

fn stock(ctx: &Ctx, code: &str) -> i64 {
    ctx.pos.find_by_code(code).unwrap().unwrap().stock
}

fn add_product(ui: &AppWindow, cat: i32, name: &str, cost: &str, price: &str, opening: &str, low: &str) -> SharedString {
    let p = ui.global::<ProductsData>();
    p.invoke_new_product();
    let mut f = p.get_form();
    f.category = cat;
    f.code = p.invoke_suggest_code(cat);
    f.name = s(name);
    f.cost = s(cost);
    f.price = s(price);
    f.stock = s(opening);
    f.low = s(low);
    assert!(p.invoke_save(f.clone()), "saving {name}: {}", p.get_form_error());
    f.code
}

fn sell_at_counter(ui: &AppWindow, entries: &[&str], form: PayForm) -> bool {
    let c = ui.global::<CounterData>();
    for e in entries {
        c.invoke_submit_query(s(e));
    }
    c.invoke_open_pay();
    let ok = c.invoke_pay(form);
    if ok {
        c.invoke_new_sale();
    }
    ok
}

fn cash(amount: &str) -> PayForm {
    PayForm { method: 0, cash: s(amount), ..Default::default() }
}

#[test]
fn full_shop_day() {
    let (ui, ctx, dir) = start();
    let app = ui.global::<App>();

    // ---------------------------------------------------------- 1. first-run setup
    let setup = ui.global::<SetupData>();
    let mut form = SetupForm {
        shop_name: s("Test EV Showroom"),
        currency: s("Rs"),
        decimals: 0,
        tax_rate: s("0"),
        tax_inclusive: true,
        owner_name: s("Owner"),
        pin: s("1234"),
        pin2: s("9999"),
        demo: false,
        ..Default::default()
    };
    assert_eq!(setup.invoke_submit(form.clone()), "The two PINs don't match.");
    form.pin2 = s("1234");
    assert_eq!(setup.invoke_submit(form), "");
    assert!(app.get_setup_done() && app.get_logged_in());
    assert_eq!(app.get_shop_name(), "Test EV Showroom");
    assert!(app.get_perms().settings && app.get_perms().view_cost);

    // ---------------------------------------------------------- 2. products with staff-entered codes
    navigate(&ctx, Page::Products);
    let p = ui.global::<ProductsData>();
    assert!(p.invoke_save_category(0, s("Brakes"), s("BRK")));
    assert!(p.invoke_save_category(0, s("Accessories"), s("ACC")));
    // Categories are listed alphabetically: 0 = Accessories, 1 = Brakes
    let brake = add_product(&ui, 1, "Front Brake Pads", "900", "1600", "10", "3");
    assert_eq!(brake, "BRK-001", "suggested code uses the category prefix");
    let helmet = add_product(&ui, 0, "Full Face Helmet", "2800", "4500", "5", "2");
    assert_eq!(helmet, "ACC-001");
    let row = product(&ui, "BRK-001");
    assert_eq!((row.stock, row.level, row.price.as_str()), (10, 0, "Rs 1,600"));
    assert_eq!(row.margin, "43.75%");

    // Duplicate code is refused (any capitalisation)
    p.invoke_new_product();
    let mut dup = p.get_form();
    dup.code = s("brk-001");
    dup.name = s("Copy");
    dup.price = s("10");
    assert!(!p.invoke_save(dup));
    assert!(p.get_form_error().contains("already used"), "{}", p.get_form_error());
    // Staff can type their own code
    p.invoke_new_product();
    let mut own = p.get_form();
    own.code = s("5501");
    own.name = s("Side Mirror");
    own.price = s("1200");
    own.cost = s("650");
    own.stock = s("4");
    assert!(p.invoke_save(own));
    assert_eq!(product(&ui, "5501").stock, 4);

    // ---------------------------------------------------------- 3. receive stock: stock up, average cost
    p.invoke_open_receive();
    p.invoke_receive_add(row.id);
    p.invoke_receive_set(0, s("5"), s("1000"));
    assert_eq!(p.get_receive_total(), "Rs 5,000");
    assert!(p.invoke_receive_save(-1, s("Supplier bill 77")), "{}", p.get_receive_error());
    assert_eq!(stock(&ctx, "BRK-001"), 15);
    assert_eq!(ctx.pos.find_by_code("BRK-001").unwrap().unwrap().cost, 93_333, "(10×900 + 5×1000) / 15");

    // ---------------------------------------------------------- 4. counter sale with discount and change
    navigate(&ctx, Page::Counter);
    let c = ui.global::<CounterData>();
    c.invoke_submit_query(s("3*BRK-001"));
    let cart = all(c.get_cart());
    assert_eq!((cart.len(), cart[0].qty), (1, 3));
    assert_eq!(c.get_total(), "Rs 4,800");
    c.set_discount_text(s("300"));
    c.invoke_set_discount(s("300"));
    assert_eq!(c.get_total(), "Rs 4,500");
    c.invoke_open_pay();
    assert!(c.get_show_pay());
    assert_eq!(c.invoke_pay_preview(cash("5000")), "Change to give: Rs 500");
    assert_eq!(c.invoke_pay_preview(cash("4000")), "Short by Rs 500", "walk-in can't leave a balance");
    assert!(!c.invoke_pay(cash("4000")), "short payment without a customer is refused");
    assert!(c.invoke_pay(cash("5000")), "{}", c.get_pay_error());
    assert!(c.get_show_done());
    assert_eq!(c.get_done_change(), "Rs 500");
    c.invoke_new_sale();
    assert_eq!(all(c.get_cart()).len(), 0);
    assert_eq!(stock(&ctx, "BRK-001"), 12, "stock goes down when you sell");

    // ---------------------------------------------------------- 5. bank sale takes stock into the red
    assert!(sell_at_counter(
        &ui,
        &["9*BRK-001"],
        PayForm { method: 1, bank: s("14400"), bank_name: s("HBL"), reference: s("TRX1"), ..Default::default() }
    ));
    assert_eq!(stock(&ctx, "BRK-001"), 3);
    navigate(&ctx, Page::Products);
    let red = product(&ui, "BRK-001");
    assert_eq!(red.level, 2, "3 left with red-at 3 → red");
    p.set_level_filter(1);
    p.invoke_refresh();
    let low: Vec<_> = all(p.get_rows()).into_iter().map(|r| r.code.to_string()).collect();
    assert!(low.contains(&"BRK-001".to_string()), "low stock filter shows it: {low:?}");
    p.set_level_filter(0);
    navigate(&ctx, Page::Dashboard);
    assert_eq!(ui.global::<DashData>().get_stats().low_count, 1);
    assert_eq!(ui.get_low_badge(), "1", "sidebar badge on Parts & Stock");

    // ---------------------------------------------------------- 6. overselling is blocked
    navigate(&ctx, Page::Counter);
    c.invoke_submit_query(s("5*BRK-001"));
    assert!(c.get_message().contains("Only 3"), "{}", c.get_message());
    assert_eq!(all(c.get_cart()).len(), 0);

    // ---------------------------------------------------------- 7. hold and resume a bill
    c.invoke_submit_query(s("ACC-001"));
    c.invoke_hold();
    assert_eq!((all(c.get_cart()).len(), all(c.get_held()).len()), (0, 1));
    c.invoke_resume(0);
    assert_eq!((all(c.get_cart()).len(), all(c.get_held()).len()), (1, 0));
    c.invoke_clear();
    assert_eq!(stock(&ctx, "ACC-001"), 5, "holding doesn't touch stock");

    // ---------------------------------------------------------- 8. adjust stock + history
    navigate(&ctx, Page::Products);
    p.invoke_open_adjust(red.id);
    assert!(p.invoke_adjust(0, s("7"), s("Found / recount"), s("")), "{}", p.get_adjust_error());
    assert_eq!(stock(&ctx, "BRK-001"), 10);
    p.invoke_open_adjust(red.id);
    assert!(!p.invoke_adjust(1, s("50"), s("Damaged"), s("")), "can't remove more than in stock");
    p.invoke_open_history(red.id);
    let hist = all(p.get_history());
    let reasons: Vec<String> = hist.iter().map(|h| h.reason.to_string()).collect();
    for r in ["Opening stock", "Received from supplier", "Sold", "Found / recount"] {
        assert!(reasons.iter().any(|x| x == r), "history has {r}: {reasons:?}");
    }

    // ---------------------------------------------------------- 9. automatic profit sheet + Excel
    navigate(&ctx, Page::Reports);
    let r = ui.global::<ReportsData>();
    r.set_tab(0);
    r.invoke_set_range(0); // today
    assert_eq!(r.get_revenue(), "Rs 18,900", "4,500 + 14,400");
    assert_eq!(r.get_cost(), "Rs 11,200", "12 × 933.33");
    assert_eq!(r.get_profit(), "Rs 7,700");
    let items = all(r.get_items());
    let pads = items.iter().find(|i| i.name == "Front Brake Pads").expect("item in profit sheet");
    assert_eq!((pads.qty.as_str(), pads.profit.as_str()), ("12", "Rs 7,700"));
    let secs = all(r.get_sections());
    assert!(secs.iter().any(|s| s.name == "Parts & accessories" && s.revenue == "Rs 18,900"));
    let (name, csv) = crate::screens::reports::profit_csv(&ctx).unwrap();
    assert!(name.starts_with("profit-sheet-") && name.ends_with(".csv"));
    assert!(csv.contains("Front Brake Pads") && csv.contains("GRAND TOTAL"), "{csv}");
    assert!(csv.contains(",18900,") && csv.contains(",7700,"), "{csv}");
    let (_, sales_csv) = crate::screens::reports::sales_csv(&ctx).unwrap();
    assert_eq!(sales_csv.matches("INV-").count(), 2, "{sales_csv}");
    let (_, stock_csv) = crate::screens::reports::stock_csv(&ctx).unwrap();
    assert!(stock_csv.contains("BRK-001,Front Brake Pads,Brakes,10,3"), "{stock_csv}");

    // ---------------------------------------------------------- 10. return an item from a past bill
    r.set_tab(1);
    r.invoke_refresh();
    let sales = all(r.get_sales());
    assert_eq!(sales.len(), 2);
    let first = sales.iter().find(|s| s.invoice == "INV-000001").unwrap();
    r.invoke_open_sale(first.id);
    assert!(r.get_show_sale() && r.get_sale_can_return());
    r.invoke_set_return_qty(0, s("1"));
    assert_eq!(r.get_return_total(), "Rs 1,500", "discount is shared across the 3 items");
    assert!(r.invoke_do_return(0, s("Wrong size")), "{}", r.get_return_error());
    assert_eq!(stock(&ctx, "BRK-001"), 11, "returned item goes back into stock");
    r.set_show_sale(false);
    r.set_tab(0);
    r.invoke_refresh();
    assert_eq!(r.get_revenue(), "Rs 17,400", "profit sheet takes the return off");

    // ---------------------------------------------------------- 11. customer on credit, then pays
    let cid = ui.global::<CustomerSearch>().invoke_create(s("Ali Khan"), s("0300-1111111"), s(""), s(""));
    assert!(cid > 0);
    navigate(&ctx, Page::Counter);
    c.set_customer_id(cid);
    c.set_customer_name(s("Ali Khan"));
    c.invoke_customer_changed(cid);
    c.invoke_submit_query(s("2*BRK-001"));
    c.invoke_open_pay();
    assert_eq!(c.invoke_pay_preview(cash("1000")), "Balance on account: Rs 2,200");
    assert!(c.invoke_pay(cash("1000")), "{}", c.get_pay_error());
    assert!(c.get_done_has_balance());
    assert_eq!(c.get_done_balance(), "Rs 2,200");
    c.invoke_new_sale();
    navigate(&ctx, Page::Customers);
    let cu = ui.global::<CustomersData>();
    let ali = all(cu.get_rows()).into_iter().find(|r| r.name == "Ali Khan").unwrap();
    assert!(ali.owing && ali.balance == "Rs 2,200");
    cu.set_filter(1);
    cu.invoke_refresh();
    assert_eq!(all(cu.get_rows()).len(), 1, "'Owe money' filter");
    cu.set_filter(0);
    cu.invoke_open(cid);
    assert!(!cu.invoke_receive(0, s("5000"), s("")), "can't take more than owed");
    assert!(cu.invoke_receive(1, s("2200"), s("Meezan")), "{}", cu.get_pay_error());
    assert!(!cu.get_detail().owing);
    assert_eq!(cu.get_detail().balance, "Rs 0");
    assert_eq!(stock(&ctx, "BRK-001"), 9);

    // ---------------------------------------------------------- 12. workshop service list (Settings)
    navigate(&ctx, Page::Settings);
    let st = ui.global::<SettingsData>();
    assert!(st.invoke_save_service(0, s("SRV-01"), s("Periodic Service"), s("1500"), true), "{}", st.get_service_error());

    // ---------------------------------------------------------- 13. vehicles: models, bikes, sale with warranty
    navigate(&ctx, Page::Vehicles);
    let v = ui.global::<VehiclesData>();
    assert!(v.invoke_save_model(ModelForm {
        id: 0,
        name: s("Spark S1"),
        brand: s("Spark"),
        price: s("285000"),
        cost: s("240000"),
        vw: s("12"),
        bw: s("24"),
        mw: s("24"),
        km: s("20000"),
        free: s("3"),
        active: true,
    }));
    for (chassis, motor) in [("SPK-0001", "MT-1"), ("SPK-0002", "MT-2")] {
        v.invoke_new_bike();
        let mut bf = v.get_bike_form();
        bf.model = 0;
        let prices = all(v.invoke_model_prices(0));
        assert_eq!(prices, vec![s("285000"), s("240000")]);
        bf.price = prices[0].clone();
        bf.cost = prices[1].clone();
        bf.chassis = s(chassis);
        bf.motor = s(motor);
        bf.battery = s("BT-9");
        bf.color = s("White");
        assert!(v.invoke_save_bike(bf), "{}", v.get_bike_error());
    }
    v.invoke_new_bike();
    let mut dupe = v.get_bike_form();
    dupe.model = 0;
    dupe.chassis = s("spk-0001");
    assert!(!v.invoke_save_bike(dupe), "same chassis twice is refused");
    v.set_tab(0);
    v.set_status_filter(0);
    v.invoke_refresh();
    let bikes = all(v.get_rows());
    assert_eq!(bikes.len(), 2);
    assert!(bikes.iter().all(|b| b.status == "In stock"));
    let bike1 = bikes.iter().find(|b| b.chassis == "SPK-0001").unwrap().id;
    let bike2 = bikes.iter().find(|b| b.chassis == "SPK-0002").unwrap().id;

    v.invoke_open_sell(bike1);
    assert!(v.get_show_sell());
    assert_eq!(v.get_sell_price(), "285000");
    let helmet_id = product(&ui, "ACC-001").id;
    v.invoke_sell_add(helmet_id);
    let sell = SellForm {
        customer_id: cid,
        price: s("285000"),
        cash: s("100000"),
        bank: s("189500"),
        bank_name: s("HBL"),
        reference: s("T-55"),
        reg: s("LEA-26-1"),
        ..Default::default()
    };
    let summary = all(v.invoke_sell_summary(sell.clone()));
    let total = summary.iter().find(|i| i.label == "Total").unwrap();
    assert_eq!(total.value, "Rs 289,500", "bike + helmet");
    assert_eq!(summary.last().unwrap().value, "Rs 0", "nothing left to pay");
    assert!(v.invoke_complete_sale(sell), "{}", v.get_sell_error());
    let warranty: Vec<String> = all(v.get_done_warranty()).iter().map(|w| w.label.to_string()).collect();
    assert_eq!(warranty, vec!["Vehicle warranty", "Battery warranty", "Motor warranty", "Free service coupons"]);
    assert!(!v.get_done_has_balance());
    let sold = ctx.pos.get_vehicle(bike1 as i64).unwrap();
    assert_eq!(sold.status, pos_core::VehicleStatus::Sold);
    assert_eq!((sold.coupons_left, sold.registration_no.as_str()), (3, "LEA-26-1"));
    assert_eq!(stock(&ctx, "ACC-001"), 4, "helmet sold with the bike left stock");

    // ---------------------------------------------------------- 14. booking with advance, then delivery
    v.invoke_open_book(bike2);
    assert!(
        v.invoke_save_booking(BookForm { customer_id: cid, model: -1, agreed: s("280000"), cash: s("50000"), ..Default::default() }),
        "{}",
        v.get_book_error()
    );
    assert_eq!(ctx.pos.get_vehicle(bike2 as i64).unwrap().status, pos_core::VehicleStatus::Booked);
    v.set_tab(1);
    v.set_booking_filter(0);
    v.invoke_refresh();
    let bookings = all(v.get_bookings());
    assert_eq!(bookings.len(), 1);
    assert_eq!((bookings[0].advance.as_str(), bookings[0].balance.as_str()), ("Rs 50,000", "Rs 230,000"));
    v.invoke_deliver_booking(bookings[0].id);
    assert!(v.get_show_sell() && v.get_sell_has_advance());
    assert_eq!(v.get_sell_price(), "280000", "agreed price is used");
    assert!(v.invoke_complete_sale(SellForm {
        customer_id: cid,
        price: s("280000"),
        bank: s("230000"),
        bank_name: s("UBL"),
        ..Default::default()
    }));
    assert!(!v.get_done_has_balance(), "advance + bank = full price");
    v.invoke_refresh();
    assert_eq!(all(v.get_bookings()).len(), 0, "no open bookings left");

    // ---------------------------------------------------------- 15. workshop: coupon service, part, deliver
    navigate(&ctx, Page::Workshop);
    let w = ui.global::<WorkshopData>();
    w.invoke_open_new();
    w.invoke_search_bikes(s("SPK-0001"));
    let hits = all(w.get_bike_hits());
    assert_eq!((hits.len(), hits[0].coupons), (1, 3));
    w.invoke_pick_bike(hits[0].id);
    assert_eq!(w.get_new_coupons(), 3);
    assert!(
        w.invoke_create_job(NewJobForm { odometer: s("500"), complaint: s("First free service"), tech_index: -1, coupon: true, ..Default::default() }),
        "{}",
        w.get_new_error()
    );
    assert!(w.get_show_job());
    w.invoke_add_service(0, s(""), s("1500"));
    w.invoke_add_part(product(&ui, "BRK-001").id);
    assert_eq!(w.get_job_error(), "");
    let job = w.get_job();
    assert_eq!((job.labor_total.as_str(), job.parts_total.as_str()), ("Rs 0", "Rs 1,600"), "coupon makes labour free");
    assert_eq!(stock(&ctx, "BRK-001"), 8, "part taken from stock when added to the job");
    w.invoke_set_status(3);
    let cols = all(w.get_columns());
    assert_eq!(all(cols[3].jobs.clone()).len(), 1, "job shows under In progress");
    w.invoke_open_deliver();
    assert_eq!(w.get_deliver_total(), "Rs 1,600");
    assert!(w.invoke_deliver(DeliverForm { cash: s("1600"), ..Default::default() }), "{}", w.get_deliver_error());
    assert!(!w.get_show_job());
    assert_eq!(stock(&ctx, "BRK-001"), 8, "delivery doesn't take the part twice");
    assert_eq!(ctx.pos.get_vehicle(bike1 as i64).unwrap().coupons_left, 2);

    // A customer's own bike (bought elsewhere); job cancelled → part back in stock
    w.invoke_open_new();
    assert!(w.invoke_add_own_bike(cid, s("Other EV 60V"), s(""), s("LEB-9"), s("Red")), "{}", w.get_new_error());
    assert!(w.invoke_create_job(NewJobForm { complaint: s("Brakes squeak"), tech_index: -1, ..Default::default() }));
    w.invoke_add_part(product(&ui, "BRK-001").id);
    assert_eq!(stock(&ctx, "BRK-001"), 7);
    w.invoke_cancel_job();
    assert_eq!(stock(&ctx, "BRK-001"), 8, "cancelling puts parts back");

    // ---------------------------------------------------------- 16. profit sheet now has all sections
    navigate(&ctx, Page::Reports);
    r.set_tab(0);
    r.invoke_refresh();
    let secs = all(r.get_sections());
    let find = |n: &str| secs.iter().find(|s| s.name == n).unwrap_or_else(|| panic!("section {n}")).clone();
    assert_eq!(find("Vehicles").revenue, "Rs 565,000");
    assert_eq!(find("Vehicles").profit, "Rs 85,000");
    assert_eq!(find("Workshop parts").revenue, "Rs 1,600");
    assert!(!all(r.get_days()).is_empty(), "day-by-day chart has data");

    // ---------------------------------------------------------- 17. cash drawer
    navigate(&ctx, Page::Cash);
    let cd = ui.global::<CashData>();
    assert!(cd.invoke_open_drawer(s("5000")), "{}", cd.get_error());
    assert!(cd.get_open() && app.get_drawer_open());
    navigate(&ctx, Page::Counter);
    assert!(sell_at_counter(&ui, &["BRK-001"], cash("2000")));
    navigate(&ctx, Page::Cash);
    assert_eq!(cd.get_expected(), "Rs 6,600", "5,000 float + 1,600 sale (change given back)");
    assert!(cd.invoke_cash_move(-1, s("200"), s("Tea")));
    assert_eq!(cd.get_expected(), "Rs 6,400");
    assert_eq!(cd.invoke_diff_text(s("6300")), "Rs 100 short");
    assert!(cd.invoke_close_drawer(s("6300"), s("")));
    assert!(!cd.get_open());
    assert_eq!(all(cd.get_history())[0].diff, "− Rs 100");

    // ---------------------------------------------------------- 18. tax added on top (17% GST)
    navigate(&ctx, Page::Settings);
    assert!(st.invoke_save_money(MoneyForm {
        currency: s("Rs"),
        decimals: 0,
        tax_rate: s("17"),
        tax_inclusive: false,
        tax_label: s("GST"),
        low_default: s("3"),
        suggest_codes: true,
        service_days: s("90"),
        ..Default::default()
    }));
    navigate(&ctx, Page::Counter);
    c.invoke_submit_query(s("BRK-001"));
    assert_eq!(c.get_total(), "Rs 1,872");
    assert!(c.get_show_tax());
    c.invoke_clear();

    // ---------------------------------------------------------- 19. staff roles: a cashier sees less
    navigate(&ctx, Page::Settings);
    assert!(st.invoke_save_staff(0, s("Hina"), 5, s(""), s("4444"), true), "{}", st.get_staff_error());
    app.invoke_logout();
    assert!(!app.get_logged_in());
    let login = ui.global::<LoginData>();
    let hina = all(login.get_users()).into_iter().find(|u| u.name == "Hina").unwrap();
    assert!(!login.invoke_attempt(hina.id, s("0000")));
    assert!(login.invoke_attempt(hina.id, s("4444")));
    let perms = app.get_perms();
    assert!(perms.counter && perms.cash && !perms.settings && !perms.view_cost && !perms.reports && !perms.discounts);
    navigate(&ctx, Page::Counter);
    c.invoke_submit_query(s("BRK-001"));
    c.invoke_set_discount(s("100"));
    c.invoke_open_pay();
    assert!(!c.invoke_pay(cash("2000")));
    assert!(c.get_pay_error().contains("discount"), "{}", c.get_pay_error());
    c.invoke_set_discount(s("0"));
    assert!(c.invoke_pay(cash("2000")), "{}", c.get_pay_error());
    c.invoke_new_sale();

    // ---------------------------------------------------------- 20. printing works for every sale
    for sale in ctx.pos.list_sales(&pos_core::SalesFilter::default()).unwrap_or_default() {
        let detail = ctx.pos.get_sale(sale.id).unwrap();
        let html = crate::print::sale_html(&ctx.pos, &detail, crate::print::Paper::A4);
        assert!(html.contains(&detail.sale.invoice_no));
    }

    drop(ui);
    let _ = std::fs::remove_dir_all(dir);
}
