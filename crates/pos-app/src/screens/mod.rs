//! Wiring for each screen: loads data into the UI and handles its buttons.

pub mod cash;
pub mod counter;
pub mod customers;
pub mod dashboard;
pub mod login;
pub mod products;
pub mod reports;
pub mod settings;
pub mod setup;
pub mod shared;
pub mod vehicles;
pub mod workshop;

use crate::ctx::Ctx;
use crate::{App, Page, Theme};
use slint::ComponentHandle;
use std::rc::Rc;

pub fn init(ctx: &Rc<Ctx>) {
    shared::init(ctx);
    setup::init(ctx);
    login::init(ctx);
    dashboard::init(ctx);
    counter::init(ctx);
    products::init(ctx);
    vehicles::init(ctx);
    workshop::init(ctx);
    customers::init(ctx);
    reports::init(ctx);
    cash::init(ctx);
    settings::init(ctx);

    let ui = ctx.ui();
    let app = ui.global::<App>();
    {
        let ctx = ctx.clone();
        app.on_navigate(move |page| navigate(&ctx, page));
    }
    {
        let ctx = ctx.clone();
        app.on_logout(move || {
            ctx.pos.logout();
            ctx.apply_session();
            login::load_users(&ctx);
        });
    }
    {
        let ctx = ctx.clone();
        app.on_toggle_theme(move || {
            let ui = ctx.ui();
            let dark = !ui.global::<Theme>().get_dark();
            ui.global::<Theme>().set_dark(dark);
            let _ = ctx.pos.set_dark_theme(dark);
        });
    }
}

pub fn navigate(ctx: &Rc<Ctx>, page: Page) {
    let ui = ctx.ui();
    ui.global::<App>().set_page(page);
    refresh(ctx, page);
}

pub fn refresh(ctx: &Rc<Ctx>, page: Page) {
    ctx.update_badges();
    match page {
        Page::Dashboard => dashboard::refresh(ctx),
        Page::Counter => counter::refresh(ctx),
        Page::Products => products::refresh(ctx),
        Page::Vehicles => vehicles::refresh(ctx),
        Page::Workshop => workshop::refresh(ctx),
        Page::Customers => customers::refresh(ctx),
        Page::Reports => reports::refresh(ctx),
        Page::Cash => cash::refresh(ctx),
        Page::Settings => settings::refresh(ctx),
    }
}
