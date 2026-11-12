//! Shared app context: the business layer, the window and small UI helpers.

use crate::{App, AppWindow, Page, Perms, Theme};
use pos_core::{Money, Perm, Pos};
use slint::{ComponentHandle, SharedString, Timer, TimerMode};
use std::path::PathBuf;
use std::time::Duration;

pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("EV_POS_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("EV Showroom POS")
}

pub struct Ctx {
    pub pos: Pos,
    ui: slint::Weak<AppWindow>,
    pub data_dir: PathBuf,
    toast_timer: Timer,
    clock_timer: Timer,
}

impl Ctx {
    pub fn new(pos: Pos, app: &AppWindow, data_dir: PathBuf) -> Self {
        Ctx { pos, ui: app.as_weak(), data_dir, toast_timer: Timer::default(), clock_timer: Timer::default() }
    }

    pub fn ui(&self) -> AppWindow {
        self.ui.upgrade().expect("main window is alive")
    }

    pub fn weak(&self) -> slint::Weak<AppWindow> {
        self.ui.clone()
    }

    pub fn money(&self, m: Money) -> SharedString {
        self.pos.money().fmt(m).into()
    }

    pub fn toast(&self, msg: impl Into<SharedString>, error: bool) {
        let ui = self.ui();
        let app = ui.global::<App>();
        app.set_toast_text(msg.into());
        app.set_toast_error(error);
        app.set_toast_visible(true);
        let weak = self.weak();
        self.toast_timer.start(TimerMode::SingleShot, Duration::from_millis(if error { 4200 } else { 2600 }), move || {
            if let Some(ui) = weak.upgrade() {
                ui.global::<App>().set_toast_visible(false);
            }
        });
    }

    pub fn ok(&self, msg: impl Into<SharedString>) {
        self.toast(msg, false);
    }

    pub fn err(&self, e: impl std::fmt::Display) {
        self.toast(e.to_string(), true);
    }

    /// Called once the window exists.
    pub fn start(&self) {
        let ui = self.ui();
        let app = ui.global::<App>();
        app.set_setup_done(self.pos.is_setup_done());
        self.apply_settings();
        self.tick_clock();
        let weak = self.weak();
        self.clock_timer.start(TimerMode::Repeated, Duration::from_secs(15), move || {
            if let Some(ui) = weak.upgrade() {
                let now = chrono::Local::now();
                ui.global::<App>().set_clock(now.format("%-I:%M %p").to_string().into());
                ui.global::<App>().set_date_long(now.format("%A, %-d %B %Y").to_string().into());
            }
        });
        crate::screens::login::load_users(self);
    }

    /// Debug builds only: `EV_POS_DEV_DEMO=1` creates a sample shop and logs in as the owner,
    /// `EV_POS_DEV_PAGE=<page>` opens a page. Used for development screenshots.
    #[cfg(debug_assertions)]
    pub fn dev_autologin(self: &std::rc::Rc<Self>) {
        if std::env::var("EV_POS_DEV_DEMO").ok().as_deref() != Some("1") {
            return;
        }
        if !self.pos.is_setup_done() {
            let settings = pos_core::Settings { shop_name: "Green Wheels EV".into(), ..Default::default() };
            let _ = self.pos.complete_setup(pos_core::SetupInput {
                settings,
                owner_name: "Adeel".into(),
                owner_pin: "1234".into(),
                demo_data: true,
            });
        } else if let Some(owner) = self.pos.login_users().ok().and_then(|u| u.into_iter().next()) {
            let _ = self.pos.login(owner.id, "1234");
        }
        self.ui().global::<App>().set_setup_done(true);
        self.apply_settings();
        self.apply_session();
        let page = match std::env::var("EV_POS_DEV_PAGE").unwrap_or_default().as_str() {
            "counter" => Page::Counter,
            "vehicles" => Page::Vehicles,
            "workshop" => Page::Workshop,
            "products" => Page::Products,
            "customers" => Page::Customers,
            "reports" => Page::Reports,
            "cash" => Page::Cash,
            "settings" => Page::Settings,
            _ => Page::Dashboard,
        };
        crate::screens::navigate(self, page);
        let ui = self.ui();
        if let Ok(cart) = std::env::var("EV_POS_DEV_CART") {
            let c = ui.global::<crate::CounterData>();
            for entry in cart.split(',') {
                c.invoke_submit_query(entry.into());
            }
        }
        if let Ok(q) = std::env::var("EV_POS_DEV_QUERY") {
            let c = ui.global::<crate::CounterData>();
            c.set_query(q.clone().into());
            c.invoke_query_changed(q.into());
        }
        if let Ok(job) = std::env::var("EV_POS_DEV_JOB") {
            if let Ok(id) = job.parse::<i32>() {
                ui.global::<crate::WorkshopData>().invoke_open_job(id);
            }
        }
        if let Ok(id) = std::env::var("EV_POS_DEV_EDIT_PRODUCT") {
            if let Ok(id) = id.parse::<i32>() {
                ui.global::<crate::ProductsData>().invoke_edit(id);
            }
        }
        if std::env::var("EV_POS_DEV_NEW_BIKE").ok().as_deref() == Some("1") {
            ui.global::<crate::VehiclesData>().invoke_new_bike();
        }
        if let Ok(bike) = std::env::var("EV_POS_DEV_SELL") {
            if let Ok(id) = bike.parse::<i32>() {
                ui.global::<crate::VehiclesData>().invoke_open_sell(id);
            }
        }
        if std::env::var("EV_POS_DEV_PAY").ok().as_deref() == Some("1") {
            self.ui().global::<crate::CounterData>().invoke_open_pay();
        }
        if let Ok(cash) = std::env::var("EV_POS_DEV_PAYNOW") {
            let c = ui.global::<crate::CounterData>();
            c.invoke_pay(crate::PayForm { method: 0, cash: cash.into(), ..Default::default() });
        }
        if std::env::var("EV_POS_DEV_DARK").ok().as_deref() == Some("1") {
            self.ui().global::<Theme>().set_dark(true);
        }
    }

    fn tick_clock(&self) {
        let now = chrono::Local::now();
        let ui = self.ui();
        ui.global::<App>().set_clock(now.format("%-I:%M %p").to_string().into());
        ui.global::<App>().set_date_long(now.format("%A, %-d %B %Y").to_string().into());
    }

    /// Shop name, theme and currency from settings.
    pub fn apply_settings(&self) {
        let s = self.pos.settings();
        let ui = self.ui();
        let app = ui.global::<App>();
        app.set_shop_name(if s.shop_name.is_empty() { "EV Showroom".into() } else { s.shop_name.clone().into() });
        app.set_currency(s.currency_symbol.clone().into());
        ui.global::<Theme>().set_dark(s.dark_theme);
    }

    /// After login: user details and what they may see.
    pub fn apply_session(&self) {
        let ui = self.ui();
        let app = ui.global::<App>();
        match self.pos.current_user() {
            Some(u) => {
                app.set_user_name(u.name.clone().into());
                app.set_user_role(u.role.label().into());
                app.set_user_initials(crate::conv::initials(&u.name).into());
                app.set_user_color((u.id % 6) as i32);
                let p = |perm| self.pos.can(perm);
                app.set_perms(Perms {
                    dashboard: p(Perm::Dashboard),
                    counter: p(Perm::Counter),
                    vehicles: p(Perm::Vehicles),
                    sell_vehicles: p(Perm::SellVehicles),
                    workshop: p(Perm::Workshop),
                    products: p(Perm::Products),
                    edit_products: p(Perm::EditProducts),
                    adjust_stock: p(Perm::AdjustStock),
                    customers: p(Perm::Customers),
                    reports: p(Perm::Reports),
                    view_cost: p(Perm::ViewCost),
                    discounts: p(Perm::Discounts),
                    refunds: p(Perm::Refunds),
                    cash: p(Perm::CashDrawer),
                    settings: p(Perm::Settings),
                    staff: p(Perm::Staff),
                });
                app.set_logged_in(true);
            }
            None => {
                app.set_logged_in(false);
                app.set_perms(Perms::default());
            }
        }
    }

    /// The first page this user is allowed to see.
    pub fn home_page(&self) -> Page {
        if self.pos.can(Perm::Dashboard) {
            Page::Dashboard
        } else if self.pos.can(Perm::Workshop) {
            Page::Workshop
        } else if self.pos.can(Perm::Counter) {
            Page::Counter
        } else {
            Page::Products
        }
    }

    pub fn update_badges(&self) {
        let ui = self.ui();
        let n = self
            .pos
            .stock_report()
            .map(|r| r.low + r.out)
            .unwrap_or(0);
        ui.set_low_badge(if n > 0 { n.to_string().into() } else { SharedString::new() });
        let open = self.pos.current_shift().ok().flatten().is_some();
        ui.global::<App>().set_drawer_open(open);
    }

    /// Open a printable receipt (thermal) or A4 invoice for a sale.
    pub fn print_sale(&self, sale_id: i64, paper: crate::print::Paper) {
        match self.pos.get_sale(sale_id) {
            Ok(sale) => {
                let html = crate::print::sale_html(&self.pos, &sale, paper);
                let name = format!("{}-{}", sale.sale.invoice_no, if paper == crate::print::Paper::A4 { "a4" } else { "receipt" });
                if let Err(e) = crate::print::open_html(&self.data_dir, &name, &html) {
                    self.err(format!("Couldn't open the print preview: {e}"));
                }
            }
            Err(e) => self.err(e),
        }
    }

    pub fn print_job(&self, job_id: i64) {
        match self.pos.get_job(job_id) {
            Ok(job) => {
                let html = crate::print::job_card_html(&self.pos, &job);
                if let Err(e) = crate::print::open_html(&self.data_dir, &format!("{}-card", job.job_no), &html) {
                    self.err(format!("Couldn't open the print preview: {e}"));
                }
            }
            Err(e) => self.err(e),
        }
    }

    pub fn on_exit(&self) {
        let dir = self.data_dir.join("backups");
        if let Err(e) = self.pos.auto_backup(&dir, 30) {
            eprintln!("Automatic backup failed: {e}");
        }
    }
}
