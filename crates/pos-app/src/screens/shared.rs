//! Customer and product search shared by many screens.

use crate::conv::{customer_hit, model, product_hit};
use crate::ctx::Ctx;
use crate::{CustomerHit, CustomerSearch, ProductLookup};
use pos_core::CustomerInput;
use slint::ComponentHandle;
use std::rc::Rc;

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let cs = ui.global::<CustomerSearch>();
    {
        let ctx = ctx.clone();
        cs.on_search(move |q| {
            let hits = ctx.pos.list_customers(&q, false, 8).unwrap_or_default();
            let rows: Vec<CustomerHit> = hits.iter().map(|c| customer_hit(&ctx, c)).collect();
            ctx.ui().global::<CustomerSearch>().set_hits(model(rows));
        });
    }
    {
        let ctx = ctx.clone();
        cs.on_create(move |name, phone, id_no, address| {
            let r = ctx.pos.save_customer(CustomerInput {
                id: None,
                name: name.to_string(),
                phone: phone.to_string(),
                id_no: id_no.to_string(),
                address: address.to_string(),
                ..Default::default()
            });
            let ui = ctx.ui();
            match r {
                Ok(id) => {
                    ui.global::<CustomerSearch>().set_error("".into());
                    ctx.ok(format!("Customer {} added", name.trim()));
                    id as i32
                }
                Err(e) => {
                    ui.global::<CustomerSearch>().set_error(e.to_string().into());
                    0
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        cs.on_get(move |id| match ctx.pos.get_customer(id as i64) {
            Ok(c) => customer_hit(&ctx, &c),
            Err(_) => CustomerHit::default(),
        });
    }

    let pl = ui.global::<ProductLookup>();
    {
        let ctx = ctx.clone();
        pl.on_search(move |q| {
            let hits = ctx.pos.search_products(&q, 8).unwrap_or_default();
            let rows = hits.iter().map(|p| product_hit(&ctx, p)).collect();
            ctx.ui().global::<ProductLookup>().set_hits(model(rows));
        });
    }
    {
        let ctx = ctx.clone();
        pl.on_by_code(move |code| match ctx.pos.find_by_code(&code) {
            Ok(Some(p)) => p.id as i32,
            _ => 0,
        });
    }
}
