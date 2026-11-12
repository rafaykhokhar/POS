// Hide the console window on Windows release builds.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

slint::include_modules!();

mod conv;
#[cfg(test)]
mod e2e_tests;
mod ctx;
mod print;
mod screens;

use std::rc::Rc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = ctx::data_dir();
    let pos = match pos_core::Pos::open(&data_dir.join("pos.db")) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Could not open the shop database in {}: {e}", data_dir.display());
            return Err(Box::new(e));
        }
    };
    let app = AppWindow::new()?;
    let ctx = Rc::new(ctx::Ctx::new(pos, &app, data_dir));
    screens::init(&ctx);
    ctx.start();
    #[cfg(debug_assertions)]
    ctx.dev_autologin();
    app.run()?;
    ctx.on_exit();
    Ok(())
}
