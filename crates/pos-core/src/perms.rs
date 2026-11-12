//! What each staff role is allowed to do.

use crate::models::Role;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Perm {
    Dashboard,
    Counter,
    Vehicles,
    SellVehicles,
    Workshop,
    /// View the products and stock list.
    Products,
    /// Add/edit products, prices and codes.
    EditProducts,
    /// Adjust stock, stocktake, receive stock.
    AdjustStock,
    Customers,
    Reports,
    /// See cost prices and profit.
    ViewCost,
    Discounts,
    Refunds,
    CashDrawer,
    Settings,
    Staff,
}

pub fn can(role: Role, perm: Perm) -> bool {
    use Perm::*;
    match role {
        Role::Owner => true,
        Role::Manager => !matches!(perm, Settings | Staff),
        Role::Sales => matches!(
            perm,
            Dashboard | Counter | Vehicles | SellVehicles | Products | Customers | Discounts | CashDrawer
        ),
        Role::Advisor => matches!(
            perm,
            Dashboard | Counter | Vehicles | Workshop | Products | Customers | Discounts | CashDrawer
        ),
        Role::Technician => matches!(perm, Workshop | Products),
        Role::Cashier => matches!(perm, Dashboard | Counter | Products | Customers | CashDrawer),
    }
}
