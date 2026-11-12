//! Business logic for the EV showroom POS: products, stock, vehicles,
//! workshop, sales, payments and reports. No UI code lives here.

pub mod db;
pub mod error;
pub mod models;
pub mod money;
pub mod perms;
mod pos;
pub mod util;

pub use error::{Error, Result};
pub use models::*;
pub use money::{Money, MoneyFmt};
pub use perms::Perm;
pub use pos::{compute_totals, Pos};
