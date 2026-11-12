//! PIN login.

use crate::conv::{initials, model};
use crate::ctx::Ctx;
use crate::{LoginData, UserTile};
use slint::ComponentHandle;
use std::rc::Rc;

pub fn load_users(ctx: &Ctx) {
    let users = ctx.pos.login_users().unwrap_or_default();
    let tiles: Vec<UserTile> = users
        .iter()
        .map(|u| UserTile {
            id: u.id as i32,
            name: u.name.clone().into(),
            role: u.role.label().into(),
            initials: initials(&u.name).into(),
            color: (u.id % 6) as i32,
        })
        .collect();
    let ui = ctx.ui();
    ui.global::<LoginData>().set_users(model(tiles));
    ui.global::<LoginData>().set_error("".into());
}

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let ctx = ctx.clone();
    ui.global::<LoginData>().on_attempt(move |id, pin| match ctx.pos.login(id as i64, &pin) {
        Ok(u) => {
            ctx.apply_session();
            super::navigate(&ctx, ctx.home_page());
            ctx.ok(format!("Welcome, {}", u.name));
            true
        }
        Err(e) => {
            ctx.ui().global::<LoginData>().set_error(e.to_string().into());
            false
        }
    });
}
