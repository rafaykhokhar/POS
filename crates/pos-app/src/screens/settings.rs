//! Settings: shop, money & tax, staff, services, backup, activity.

use crate::conv::{initials, int_in, model, money_in, strings};
use crate::ctx::Ctx;
use crate::{AuditRow, MoneyForm, SettingsData, ShopForm, StaffRow, SvcRow};
use pos_core::money::{fmt_percent, parse_percent};
use pos_core::util::pretty_ts;
use pos_core::{LaborService, Role, UserInput};
use slint::ComponentHandle;
use std::rc::Rc;

fn backups_dir(ctx: &Ctx) -> std::path::PathBuf {
    ctx.data_dir.join("backups")
}

pub fn refresh(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let d = ui.global::<SettingsData>();
    let s = ctx.pos.settings();
    if !ctx.pos.can(pos_core::Perm::Settings) && d.get_tab() < 2 {
        d.set_tab(if ctx.pos.can(pos_core::Perm::Staff) { 2 } else { 6 });
    }
    d.set_shop(ShopForm {
        name: s.shop_name.clone().into(),
        phone: s.shop_phone.clone().into(),
        address: s.shop_address.clone().into(),
        email: s.shop_email.clone().into(),
        tax_number: s.tax_number.clone().into(),
        footer: s.receipt_footer.clone().into(),
        terms: s.invoice_terms.clone().into(),
    });
    d.set_money(MoneyForm {
        currency: s.currency_symbol.clone().into(),
        decimals: if s.currency_decimals == 0 { 0 } else { 1 },
        symbol_after: s.symbol_after,
        tax_rate: fmt_percent(s.tax_rate_bp).trim_end_matches('%').to_string().into(),
        tax_inclusive: s.tax_inclusive,
        tax_label: s.tax_label.clone().into(),
        low_default: s.low_stock_default.to_string().into(),
        allow_negative: s.allow_negative_stock,
        suggest_codes: s.suggest_codes,
        service_days: s.next_service_days.to_string().into(),
    });
    d.set_role_names(strings(Role::ALL.iter().map(|r| r.label().to_string()).collect()));
    if ctx.pos.can(pos_core::Perm::Staff) {
        let users = ctx.pos.list_users(true).unwrap_or_default();
        d.set_staff(model(
            users
                .iter()
                .map(|u| StaffRow {
                    id: u.id as i32,
                    name: u.name.clone().into(),
                    initials: initials(&u.name).into(),
                    role: u.role.label().into(),
                    role_index: u.role.index() as i32,
                    phone: u.phone.clone().into(),
                    active: u.active,
                    has_pin: u.has_pin,
                })
                .collect(),
        ));
    }
    let services = ctx.pos.list_services(true).unwrap_or_default();
    d.set_services(model(
        services
            .iter()
            .map(|s| SvcRow {
                id: s.id as i32,
                code: s.code.clone().into(),
                name: s.name.clone().into(),
                price: ctx.money(s.price),
                price_plain: ctx.pos.money().edit(s.price).into(),
                active: s.active,
            })
            .collect(),
    ));
    if d.get_tab() == 5 {
        let log = ctx.pos.audit_log(500).unwrap_or_default();
        d.set_audit(model(
            log.iter()
                .map(|a| AuditRow {
                    when: pretty_ts(&a.created_at).into(),
                    user: a.user_name.clone().into(),
                    action: a.action.clone().into(),
                    detail: a.detail.clone().into(),
                })
                .collect(),
        ));
    }
    d.set_data_path(ctx.pos.db_path().map(|p| p.display().to_string()).unwrap_or_default().into());
    let bdir = backups_dir(ctx);
    d.set_backup_path(bdir.display().to_string().into());
    let latest = std::fs::read_dir(&bdir)
        .ok()
        .and_then(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.starts_with("backup-") && n.ends_with(".db"))
                .max()
        })
        .unwrap_or_else(|| "None yet (made when the app closes)".into());
    d.set_last_backup(latest.into());
    d.set_version(env!("CARGO_PKG_VERSION").into());
    d.set_error("".into());
}

pub fn init(ctx: &Rc<Ctx>) {
    let ui = ctx.ui();
    let d = ui.global::<SettingsData>();
    {
        let ctx = ctx.clone();
        d.on_refresh(move || refresh(&ctx));
    }
    {
        let ctx = ctx.clone();
        d.on_save_shop(move |f| {
            let mut s = ctx.pos.settings();
            s.shop_name = f.name.trim().to_string();
            s.shop_phone = f.phone.trim().to_string();
            s.shop_address = f.address.trim().to_string();
            s.shop_email = f.email.trim().to_string();
            s.tax_number = f.tax_number.trim().to_string();
            s.receipt_footer = f.footer.trim().to_string();
            s.invoice_terms = f.terms.trim().to_string();
            if s.shop_name.is_empty() {
                ctx.ui().global::<SettingsData>().set_error("Shop name can't be empty.".into());
                return false;
            }
            match ctx.pos.save_settings(s) {
                Ok(()) => {
                    ctx.apply_settings();
                    ctx.ok("Shop details saved");
                    true
                }
                Err(e) => {
                    ctx.ui().global::<SettingsData>().set_error(e.to_string().into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_save_money(move |f| {
            let ui = ctx.ui();
            let d = ui.global::<SettingsData>();
            let r = (|| -> Result<(), String> {
                let mut s = ctx.pos.settings();
                s.currency_symbol = f.currency.trim().to_string();
                s.currency_decimals = if f.decimals == 0 { 0 } else { 2 };
                s.symbol_after = f.symbol_after;
                s.tax_rate_bp = parse_percent(&f.tax_rate).ok_or("Tax rate must be between 0 and 100.")?;
                s.tax_inclusive = f.tax_inclusive;
                s.tax_label = if f.tax_label.trim().is_empty() { "Tax".into() } else { f.tax_label.trim().to_string() };
                s.low_stock_default = int_in(&f.low_default, "Low stock level")?;
                s.allow_negative_stock = f.allow_negative;
                s.suggest_codes = f.suggest_codes;
                s.next_service_days = int_in(&f.service_days, "Service reminder days")?;
                ctx.pos.save_settings(s).map_err(|e| e.to_string())
            })();
            match r {
                Ok(()) => {
                    ctx.apply_settings();
                    d.set_error("".into());
                    ctx.ok("Settings saved");
                    true
                }
                Err(e) => {
                    d.set_error(e.into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_new_staff(move || {
            let ui = ctx.ui();
            let d = ui.global::<SettingsData>();
            d.set_staff_form(StaffRow { id: 0, role_index: Role::Cashier.index() as i32, active: true, ..Default::default() });
            d.set_staff_error("".into());
            d.set_show_staff(true);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_edit_staff(move |id| {
            if let Ok(u) = ctx.pos.get_user(id as i64) {
                let ui = ctx.ui();
                let d = ui.global::<SettingsData>();
                d.set_staff_form(StaffRow {
                    id: u.id as i32,
                    name: u.name.clone().into(),
                    initials: initials(&u.name).into(),
                    role: u.role.label().into(),
                    role_index: u.role.index() as i32,
                    phone: u.phone.clone().into(),
                    active: u.active,
                    has_pin: u.has_pin,
                });
                d.set_staff_error("".into());
                d.set_show_staff(true);
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_save_staff(move |id, name, role, phone, pin, active| {
            let ui = ctx.ui();
            let d = ui.global::<SettingsData>();
            let role = Role::from_index(role.max(0) as usize).unwrap_or(Role::Cashier);
            let pin = if id == 0 {
                Some(pin.to_string())
            } else if pin.is_empty() {
                None
            } else {
                Some(pin.to_string())
            };
            match ctx.pos.save_user(UserInput {
                id: if id > 0 { Some(id as i64) } else { None },
                name: name.to_string(),
                role: Some(role),
                phone: phone.to_string(),
                pin,
                active: if id == 0 { true } else { active },
            }) {
                Ok(_) => {
                    d.set_show_staff(false);
                    ctx.ok("Staff saved");
                    refresh(&ctx);
                    crate::screens::login::load_users(&ctx);
                    true
                }
                Err(e) => {
                    d.set_staff_error(e.to_string().into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_new_service(move || {
            let ui = ctx.ui();
            let d = ui.global::<SettingsData>();
            let n = ctx.pos.list_services(true).map(|v| v.len()).unwrap_or(0) + 1;
            d.set_service_form(SvcRow { id: 0, code: format!("SRV-{n:02}").into(), active: true, ..Default::default() });
            d.set_service_error("".into());
            d.set_show_service(true);
        });
    }
    {
        let ctx = ctx.clone();
        d.on_edit_service(move |id| {
            let list = ctx.pos.list_services(true).unwrap_or_default();
            if let Some(s) = list.iter().find(|s| s.id == id as i64) {
                let ui = ctx.ui();
                let d = ui.global::<SettingsData>();
                d.set_service_form(SvcRow {
                    id: s.id as i32,
                    code: s.code.clone().into(),
                    name: s.name.clone().into(),
                    price: ctx.money(s.price),
                    price_plain: ctx.pos.money().edit(s.price).into(),
                    active: s.active,
                });
                d.set_service_error("".into());
                d.set_show_service(true);
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_save_service(move |id, code, name, price, active| {
            let ui = ctx.ui();
            let d = ui.global::<SettingsData>();
            let r = money_in(&price, "Price").and_then(|p| {
                ctx.pos
                    .save_service(LaborService {
                        id: id as i64,
                        code: code.to_string(),
                        name: name.to_string(),
                        price: p,
                        active: if id == 0 { true } else { active },
                    })
                    .map_err(|e| e.to_string())
            });
            match r {
                Ok(_) => {
                    d.set_show_service(false);
                    ctx.ok("Service saved");
                    refresh(&ctx);
                    true
                }
                Err(e) => {
                    d.set_service_error(e.into());
                    false
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_backup_now(move || {
            let name = format!("ev-pos-backup-{}.db", pos_core::util::today());
            let dir = dirs::document_dir().or_else(dirs::home_dir).unwrap_or_default();
            let Some(path) = rfd::FileDialog::new()
                .set_title("Save backup")
                .set_directory(&dir)
                .set_file_name(&name)
                .add_filter("Backup", &["db"])
                .save_file()
            else {
                return;
            };
            match ctx.pos.backup_to(&path) {
                Ok(()) => ctx.ok(format!("Backup saved: {}", path.file_name().and_then(|n| n.to_str()).unwrap_or(""))),
                Err(e) => ctx.err(format!("Backup failed: {e}")),
            }
        });
    }
    {
        let ctx = ctx.clone();
        d.on_open_folder(move || {
            let dir = ctx.data_dir.clone();
            #[cfg(target_os = "macos")]
            let r = std::process::Command::new("open").arg(&dir).spawn();
            #[cfg(target_os = "windows")]
            let r = std::process::Command::new("explorer").arg(&dir).spawn();
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let r = std::process::Command::new("xdg-open").arg(&dir).spawn();
            if let Err(e) = r {
                ctx.err(format!("Couldn't open the folder: {e}"));
            }
        });
    }
}
