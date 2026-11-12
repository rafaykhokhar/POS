//! Plain data types shared by the database layer and the UI.

use crate::money::Money;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};

/// Declares an enum stored in the database as a short text value.
macro_rules! str_enum {
    ($name:ident { $($var:ident => $s:literal / $label:literal),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name { $($var),* }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$var),*];
            pub fn as_str(self) -> &'static str { match self { $($name::$var => $s),* } }
            pub fn label(self) -> &'static str { match self { $($name::$var => $label),* } }
            pub fn parse(s: &str) -> Option<Self> { match s { $($s => Some($name::$var),)* _ => None } }
            pub fn index(self) -> usize { Self::ALL.iter().position(|x| *x == self).unwrap_or(0) }
            pub fn from_index(i: usize) -> Option<Self> { Self::ALL.get(i).copied() }
            pub fn labels() -> Vec<&'static str> { Self::ALL.iter().map(|v| v.label()).collect() }
        }

        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                Ok(ToSqlOutput::from(self.as_str()))
            }
        }

        impl FromSql for $name {
            fn column_result(v: ValueRef<'_>) -> FromSqlResult<Self> {
                let s = v.as_str()?;
                Self::parse(s).ok_or_else(|| {
                    FromSqlError::Other(format!("bad {} value: {}", stringify!($name), s).into())
                })
            }
        }
    };
}

str_enum!(Role {
    Owner => "owner" / "Owner",
    Manager => "manager" / "Manager",
    Sales => "sales" / "Salesperson",
    Advisor => "advisor" / "Service Advisor",
    Technician => "technician" / "Technician",
    Cashier => "cashier" / "Cashier",
});

str_enum!(ProductKind {
    Part => "part" / "Spare part",
    Accessory => "accessory" / "Accessory",
});

str_enum!(VehicleStatus {
    InStock => "in_stock" / "In stock",
    Booked => "booked" / "Booked",
    Sold => "sold" / "Sold",
    Demo => "demo" / "Demo",
    InTransit => "in_transit" / "In transit",
    External => "external" / "Customer bike",
});

str_enum!(BookingStatus {
    Open => "open" / "Open",
    Delivered => "delivered" / "Delivered",
    Cancelled => "cancelled" / "Cancelled",
});

str_enum!(JobStatus {
    Received => "received" / "Received",
    Diagnosing => "diagnosing" / "Diagnosing",
    WaitingParts => "waiting_parts" / "Waiting for parts",
    InProgress => "in_progress" / "In progress",
    Ready => "ready" / "Ready",
    Delivered => "delivered" / "Delivered",
    Cancelled => "cancelled" / "Cancelled",
});

str_enum!(SaleKind {
    Counter => "counter" / "Counter sale",
    Vehicle => "vehicle" / "Vehicle sale",
    Workshop => "workshop" / "Workshop",
    Return => "return" / "Return",
});

str_enum!(SaleStatus {
    Completed => "completed" / "Completed",
    Void => "void" / "Void",
});

str_enum!(PayMethod {
    Cash => "cash" / "Cash",
    Bank => "bank" / "Bank transfer",
});

str_enum!(ItemType {
    Product => "product" / "Product",
    Vehicle => "vehicle" / "Vehicle",
    Service => "service" / "Service",
    Custom => "custom" / "Custom",
});

str_enum!(Section {
    Vehicle => "vehicle" / "Vehicles",
    Part => "part" / "Parts & accessories",
    WorkshopParts => "workshop_parts" / "Workshop parts",
    Labor => "labor" / "Workshop labour",
    Other => "other" / "Other",
});

str_enum!(JobItemKind {
    Part => "part" / "Part",
    Service => "service" / "Service",
});

impl JobStatus {
    pub fn is_open(self) -> bool {
        !matches!(self, JobStatus::Delivered | JobStatus::Cancelled)
    }
    /// The statuses shown as columns on the workshop board.
    pub const BOARD: [JobStatus; 5] = [
        JobStatus::Received,
        JobStatus::Diagnosing,
        JobStatus::WaitingParts,
        JobStatus::InProgress,
        JobStatus::Ready,
    ];
}

// ---------------------------------------------------------------- settings

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub shop_name: String,
    pub shop_phone: String,
    pub shop_address: String,
    pub shop_email: String,
    pub tax_number: String,
    pub currency_symbol: String,
    pub currency_decimals: u8,
    pub symbol_after: bool,
    pub tax_rate_bp: i64,
    pub tax_inclusive: bool,
    pub tax_label: String,
    pub receipt_footer: String,
    pub invoice_terms: String,
    pub low_stock_default: i64,
    pub allow_negative_stock: bool,
    pub suggest_codes: bool,
    pub dark_theme: bool,
    pub next_service_days: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            shop_name: String::new(),
            shop_phone: String::new(),
            shop_address: String::new(),
            shop_email: String::new(),
            tax_number: String::new(),
            currency_symbol: "Rs".into(),
            currency_decimals: 0,
            symbol_after: false,
            tax_rate_bp: 0,
            tax_inclusive: true,
            tax_label: "Tax".into(),
            receipt_footer: "Thank you for your business!".into(),
            invoice_terms: String::new(),
            low_stock_default: 3,
            allow_negative_stock: false,
            suggest_codes: true,
            dark_theme: false,
            next_service_days: 90,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupInput {
    pub settings: Settings,
    pub owner_name: String,
    pub owner_pin: String,
    pub demo_data: bool,
}

// ---------------------------------------------------------------- people

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub role: Role,
    pub phone: String,
    pub active: bool,
    pub has_pin: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserInput {
    pub id: Option<i64>,
    pub name: String,
    pub role: Option<Role>,
    pub phone: String,
    /// `None` keeps the existing PIN. `Some("")` removes it.
    pub pin: Option<String>,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Customer {
    pub id: i64,
    pub name: String,
    pub phone: String,
    pub id_no: String,
    pub address: String,
    pub email: String,
    pub notes: String,
    pub created_at: String,
    /// Amount the customer still owes (negative = credit in their favour).
    pub balance: Money,
    pub vehicles: i64,
    pub total_spent: Money,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CustomerInput {
    pub id: Option<i64>,
    pub name: String,
    pub phone: String,
    pub id_no: String,
    pub address: String,
    pub email: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Supplier {
    pub id: i64,
    pub name: String,
    pub phone: String,
    pub address: String,
    pub notes: String,
}

// ---------------------------------------------------------------- catalogue

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub prefix: String,
    pub product_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StockLevel {
    Ok,
    /// Getting close to the low-stock level (amber).
    Warn,
    /// At or below the low-stock level (red).
    Low,
    Out,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    pub id: i64,
    pub code: String,
    pub barcode: String,
    pub name: String,
    pub category_id: Option<i64>,
    pub category_name: String,
    pub kind: ProductKind,
    pub brand: String,
    pub unit: String,
    /// Average cost per unit.
    pub cost: Money,
    pub price: Money,
    pub stock: i64,
    pub low_stock: i64,
    pub fits_models: String,
    pub location: String,
    pub serial_tracked: bool,
    pub warranty_months: i64,
    pub active: bool,
    pub notes: String,
}

impl Product {
    pub fn level(&self) -> StockLevel {
        stock_level(self.stock, self.low_stock)
    }
}

pub fn stock_level(stock: i64, low: i64) -> StockLevel {
    if stock <= 0 {
        StockLevel::Out
    } else if stock <= low {
        StockLevel::Low
    } else if low > 0 && stock <= low * 2 {
        StockLevel::Warn
    } else {
        StockLevel::Ok
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductInput {
    pub id: Option<i64>,
    pub code: String,
    pub barcode: String,
    pub name: String,
    pub category_id: Option<i64>,
    pub kind: ProductKind,
    pub brand: String,
    pub unit: String,
    pub cost: Money,
    pub price: Money,
    /// Opening stock; only used when creating a product.
    pub stock: i64,
    pub low_stock: i64,
    pub fits_models: String,
    pub location: String,
    pub serial_tracked: bool,
    pub warranty_months: i64,
    pub notes: String,
    pub active: bool,
}

impl Default for ProductInput {
    fn default() -> Self {
        ProductInput {
            id: None,
            code: String::new(),
            barcode: String::new(),
            name: String::new(),
            category_id: None,
            kind: ProductKind::Part,
            brand: String::new(),
            unit: "pcs".into(),
            cost: 0,
            price: 0,
            stock: 0,
            low_stock: 3,
            fits_models: String::new(),
            location: String::new(),
            serial_tracked: false,
            warranty_months: 0,
            notes: String::new(),
            active: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LevelFilter {
    #[default]
    All,
    /// Low or out of stock.
    Low,
    Out,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProductFilter {
    pub search: String,
    pub category_id: Option<i64>,
    pub level: LevelFilter,
    pub kind: Option<ProductKind>,
    pub include_inactive: bool,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LaborService {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub price: Money,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VehicleModel {
    pub id: i64,
    pub name: String,
    pub brand: String,
    pub price: Money,
    pub cost: Money,
    pub vehicle_warranty_months: i64,
    pub battery_warranty_months: i64,
    pub motor_warranty_months: i64,
    pub warranty_km: i64,
    pub free_services: i64,
    pub active: bool,
    pub in_stock: i64,
}

// ---------------------------------------------------------------- stock

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockMove {
    pub id: i64,
    pub change: i64,
    pub reason: String,
    pub reference: String,
    pub note: String,
    pub unit_cost: Money,
    pub user_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReceiveLine {
    pub product_id: i64,
    pub qty: i64,
    pub unit_cost: Money,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReceiveInput {
    pub supplier_id: Option<i64>,
    pub items: Vec<ReceiveLine>,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PurchaseSummary {
    pub id: i64,
    pub po_no: String,
    pub supplier_name: String,
    pub total: Money,
    pub items: i64,
    pub created_at: String,
}

// ---------------------------------------------------------------- vehicles

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vehicle {
    pub id: i64,
    pub model_id: Option<i64>,
    pub model_name: String,
    pub chassis_no: String,
    pub motor_no: String,
    pub battery_serial: String,
    pub color: String,
    pub year: i64,
    pub cost: Money,
    pub price: Money,
    pub status: VehicleStatus,
    pub customer_id: Option<i64>,
    pub customer_name: String,
    pub customer_phone: String,
    pub registration_no: String,
    pub sold_at: String,
    pub sale_id: Option<i64>,
    pub invoice_no: String,
    pub vehicle_warranty_until: String,
    pub battery_warranty_until: String,
    pub motor_warranty_until: String,
    pub warranty_km: i64,
    pub notes: String,
    pub received_at: String,
    pub coupons_left: i64,
}

impl Vehicle {
    /// Short label such as "Volt X1 · ABC-123".
    pub fn label(&self) -> String {
        let id = if !self.registration_no.is_empty() {
            self.registration_no.clone()
        } else {
            self.chassis_no.clone()
        };
        if id.is_empty() {
            self.model_name.clone()
        } else {
            format!("{} · {}", self.model_name, id)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VehicleInput {
    pub id: Option<i64>,
    pub model_id: Option<i64>,
    pub model_name: String,
    pub chassis_no: String,
    pub motor_no: String,
    pub battery_serial: String,
    pub color: String,
    pub year: i64,
    pub cost: Money,
    pub price: Money,
    pub status: VehicleStatus,
    pub registration_no: String,
    pub notes: String,
    pub customer_id: Option<i64>,
}

impl Default for VehicleInput {
    fn default() -> Self {
        VehicleInput {
            id: None,
            model_id: None,
            model_name: String::new(),
            chassis_no: String::new(),
            motor_no: String::new(),
            battery_serial: String::new(),
            color: String::new(),
            year: 0,
            cost: 0,
            price: 0,
            status: VehicleStatus::InStock,
            registration_no: String::new(),
            notes: String::new(),
            customer_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VehicleFilter {
    pub search: String,
    pub status: Option<VehicleStatus>,
    pub model_id: Option<i64>,
    /// Include customer bikes that were not sold by the shop.
    pub include_external: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coupon {
    pub id: i64,
    pub vehicle_id: i64,
    pub seq: i64,
    pub used: bool,
    pub job_no: String,
    pub used_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Booking {
    pub id: i64,
    pub booking_no: String,
    pub customer_id: i64,
    pub customer_name: String,
    pub customer_phone: String,
    pub model_id: Option<i64>,
    pub model_name: String,
    pub vehicle_id: Option<i64>,
    pub chassis_no: String,
    pub color: String,
    pub agreed_price: Money,
    pub advance: Money,
    pub status: BookingStatus,
    pub expected_date: String,
    pub notes: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BookingInput {
    pub customer_id: i64,
    pub model_id: Option<i64>,
    pub vehicle_id: Option<i64>,
    pub color: String,
    pub agreed_price: Money,
    pub expected_date: String,
    pub notes: String,
    pub advance: Vec<PaymentInput>,
}

// ---------------------------------------------------------------- workshop

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobItem {
    pub id: i64,
    pub job_id: i64,
    pub kind: JobItemKind,
    pub product_id: Option<i64>,
    pub service_id: Option<i64>,
    pub code: String,
    pub description: String,
    pub qty: i64,
    pub unit_price: Money,
    pub unit_cost: Money,
    /// Free of charge for the customer (warranty or free-service coupon).
    pub free: bool,
}

impl JobItem {
    pub fn charge(&self) -> Money {
        if self.free {
            0
        } else {
            self.unit_price * self.qty
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobCard {
    pub id: i64,
    pub job_no: String,
    pub vehicle_id: i64,
    pub vehicle_label: String,
    pub model_name: String,
    pub chassis_no: String,
    pub registration_no: String,
    pub customer_id: Option<i64>,
    pub customer_name: String,
    pub customer_phone: String,
    pub odometer: i64,
    pub battery_note: String,
    pub complaint: String,
    pub diagnosis: String,
    pub status: JobStatus,
    pub technician_id: Option<i64>,
    pub technician_name: String,
    pub is_warranty: bool,
    pub coupon_seq: i64,
    pub approved: bool,
    pub next_service_date: String,
    pub next_service_km: i64,
    pub sale_id: Option<i64>,
    pub invoice_no: String,
    pub created_at: String,
    pub updated_at: String,
    pub delivered_at: String,
    pub parts_total: Money,
    pub labor_total: Money,
    pub items: Vec<JobItem>,
}

impl JobCard {
    pub fn total(&self) -> Money {
        self.parts_total + self.labor_total
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JobInput {
    pub vehicle_id: i64,
    pub customer_id: Option<i64>,
    pub odometer: i64,
    pub battery_note: String,
    pub complaint: String,
    pub technician_id: Option<i64>,
    pub is_warranty: bool,
    pub use_coupon: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JobUpdate {
    pub odometer: i64,
    pub battery_note: String,
    pub complaint: String,
    pub diagnosis: String,
    pub technician_id: Option<i64>,
    pub is_warranty: bool,
    pub approved: bool,
    pub next_service_date: String,
    pub next_service_km: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JobFilter {
    pub open_only: bool,
    pub status: Option<JobStatus>,
    pub search: String,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDue {
    pub vehicle_id: i64,
    pub vehicle_label: String,
    pub customer_name: String,
    pub customer_phone: String,
    pub due_date: String,
    pub due_km: i64,
    pub last_job_no: String,
    pub overdue: bool,
}

// ---------------------------------------------------------------- sales

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CartLine {
    pub item_type: ItemType,
    pub ref_id: Option<i64>,
    pub code: String,
    pub description: String,
    pub qty: i64,
    pub unit_price: Money,
    pub line_discount: Money,
    pub serial_no: String,
    pub free: bool,
}

impl CartLine {
    pub fn gross(&self) -> Money {
        if self.free {
            0
        } else {
            self.unit_price * self.qty
        }
    }
    pub fn net(&self) -> Money {
        let g = self.gross();
        g - self.line_discount.clamp(0, g.max(0))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentInput {
    pub method: PayMethod,
    pub amount: Money,
    pub bank_name: String,
    pub reference: String,
    pub note: String,
}

impl PaymentInput {
    pub fn cash(amount: Money) -> Self {
        PaymentInput {
            method: PayMethod::Cash,
            amount,
            bank_name: String::new(),
            reference: String::new(),
            note: String::new(),
        }
    }
    pub fn bank(amount: Money, bank: &str, reference: &str) -> Self {
        PaymentInput {
            method: PayMethod::Bank,
            amount,
            bank_name: bank.into(),
            reference: reference.into(),
            note: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckoutInput {
    pub kind: SaleKind,
    pub customer_id: Option<i64>,
    pub salesperson_id: Option<i64>,
    pub items: Vec<CartLine>,
    pub discount: Money,
    pub payments: Vec<PaymentInput>,
    pub notes: String,
    pub booking_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct Totals {
    /// Sum of qty × price before any discount.
    pub gross: Money,
    pub line_discounts: Money,
    /// Bill-level discount actually applied.
    pub discount: Money,
    pub tax: Money,
    pub total: Money,
    pub items: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaleReceipt {
    pub sale_id: i64,
    pub invoice_no: String,
    pub total: Money,
    pub paid: Money,
    pub balance: Money,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaleSummary {
    pub id: i64,
    pub invoice_no: String,
    pub kind: SaleKind,
    pub status: SaleStatus,
    pub customer_id: Option<i64>,
    pub customer_name: String,
    pub user_name: String,
    pub total: Money,
    pub paid: Money,
    pub profit: Money,
    pub items: i64,
    pub created_at: String,
}

impl SaleSummary {
    pub fn balance(&self) -> Money {
        self.total - self.paid
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaleItem {
    pub id: i64,
    pub item_type: ItemType,
    pub product_id: Option<i64>,
    pub vehicle_id: Option<i64>,
    pub code: String,
    pub description: String,
    pub qty: i64,
    pub unit_price: Money,
    pub line_discount: Money,
    pub net_amount: Money,
    pub cost_amount: Money,
    pub free: bool,
    pub serial_no: String,
    pub warranty_until: String,
    pub returned_qty: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payment {
    pub id: i64,
    pub sale_id: Option<i64>,
    pub booking_id: Option<i64>,
    pub reference_no: String,
    pub method: PayMethod,
    pub amount: Money,
    pub bank_name: String,
    pub reference: String,
    pub note: String,
    pub user_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaleDetail {
    pub sale: SaleSummary,
    pub subtotal: Money,
    pub discount: Money,
    pub tax: Money,
    pub notes: String,
    pub salesperson_name: String,
    pub customer_phone: String,
    pub original_invoice: String,
    pub items: Vec<SaleItem>,
    pub payments: Vec<Payment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SalesFilter {
    /// Inclusive start date, `YYYY-MM-DD` (empty = no limit).
    pub from: String,
    /// Inclusive end date, `YYYY-MM-DD` (empty = no limit).
    pub to: String,
    pub kind: Option<SaleKind>,
    pub search: String,
    pub customer_id: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomerDetail {
    pub customer: Customer,
    pub vehicles: Vec<Vehicle>,
    pub sales: Vec<SaleSummary>,
    pub jobs: Vec<JobCard>,
    pub payments: Vec<Payment>,
    pub bookings: Vec<Booking>,
}

// ---------------------------------------------------------------- reports

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayTotal {
    pub date: String,
    pub label: String,
    pub total: Money,
    pub profit: Money,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dashboard {
    pub sales_today: Money,
    pub count_today: i64,
    pub profit_today: Money,
    pub cash_today: Money,
    pub bank_today: Money,
    pub sales_month: Money,
    pub profit_month: Money,
    pub vehicles_in_stock: i64,
    pub vehicles_sold_month: i64,
    pub open_jobs: i64,
    pub ready_jobs: i64,
    pub low_stock_count: i64,
    pub out_of_stock_count: i64,
    pub receivables: Money,
    pub open_bookings: i64,
    pub last7: Vec<DayTotal>,
    pub recent_sales: Vec<SaleSummary>,
    pub low_stock: Vec<Product>,
    pub jobs_by_status: Vec<(JobStatus, i64)>,
    pub service_due: Vec<ServiceDue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfitSection {
    pub section: Section,
    pub qty: i64,
    pub revenue: Money,
    pub cost: Money,
    pub profit: Money,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfitItem {
    pub section: Section,
    pub code: String,
    pub name: String,
    pub qty: i64,
    pub revenue: Money,
    pub cost: Money,
    pub profit: Money,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfitSheet {
    pub from: String,
    pub to: String,
    pub revenue: Money,
    pub cost: Money,
    pub profit: Money,
    pub discounts: Money,
    pub tax: Money,
    pub warranty_cost: Money,
    pub sales_count: i64,
    pub returns: Money,
    pub sections: Vec<ProfitSection>,
    pub items: Vec<ProfitItem>,
    pub by_day: Vec<DayTotal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SalesSummary {
    pub total: Money,
    pub count: i64,
    pub by_method: Vec<(PayMethod, Money)>,
    pub by_kind: Vec<(SaleKind, i64, Money)>,
    pub by_staff: Vec<(String, i64, Money)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StockReport {
    pub products: i64,
    pub units: i64,
    pub value_cost: Money,
    pub value_retail: Money,
    pub low: i64,
    pub out: i64,
    pub vehicles: i64,
    pub vehicles_value: Money,
}

// ---------------------------------------------------------------- cash drawer

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftSummary {
    pub id: i64,
    pub opened_by: String,
    pub opened_at: String,
    pub opening_cash: Money,
    pub cash_sales: Money,
    pub cash_in: Money,
    pub cash_out: Money,
    pub bank_total: Money,
    pub expected: Money,
    pub counted: Option<Money>,
    pub closed_by: String,
    pub closed_at: String,
    pub notes: String,
}

impl ShiftSummary {
    pub fn difference(&self) -> Option<Money> {
        self.counted.map(|c| c - self.expected)
    }
    pub fn is_open(&self) -> bool {
        self.closed_at.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashMove {
    pub id: i64,
    pub amount: Money,
    pub reason: String,
    pub user_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: i64,
    pub user_name: String,
    pub action: String,
    pub detail: String,
    pub created_at: String,
}
