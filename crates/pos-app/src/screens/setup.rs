//! First-run setup.

use crate::ctx::Ctx;
use crate::{App, SetupData};
use pos_core::money::parse_percent;
use pos_core::{Settings, SetupInput};
use slint::{ComponentHandle, SharedString};
use std::rc::Rc;

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let ctx = ctx.clone();
    ui.global::<SetupData>().on_submit(move |f| -> SharedString {
        if f.pin != f.pin2 {
            return "The two PINs don't match.".into();
        }
        let Some(tax) = parse_percent(&f.tax_rate) else {
            return "Tax rate must be a number between 0 and 100.".into();
        };
        let settings = Settings {
            shop_name: f.shop_name.trim().to_string(),
            shop_phone: f.phone.trim().to_string(),
            shop_address: f.address.trim().to_string(),
            currency_symbol: f.currency.trim().to_string(),
            currency_decimals: f.decimals.clamp(0, 2) as u8,
            tax_rate_bp: tax,
            tax_inclusive: f.tax_inclusive,
            ..Default::default()
        };
        let r = ctx.pos.complete_setup(SetupInput {
            settings,
            owner_name: f.owner_name.to_string(),
            owner_pin: f.pin.to_string(),
            demo_data: f.demo,
        });
        match r {
            Ok(()) => {
                let ui = ctx.ui();
                ctx.apply_settings();
                ctx.apply_session();
                ui.global::<App>().set_setup_done(true);
                super::navigate(&ctx, ctx.home_page());
                ctx.ok("Your shop is ready. Welcome!");
                SharedString::new()
            }
            Err(e) => e.to_string().into(),
        }
    });
}
