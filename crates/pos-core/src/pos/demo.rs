//! Sample data so the shop can explore the app before entering real stock.

use super::sales::SaleOpts;
use super::Pos;
use crate::error::Result;
use crate::models::*;
use crate::util::{add_days, today};

const RS: i64 = 100; // one currency unit in minor units

impl Pos {
    pub fn load_demo_data(&self) -> Result<()> {
        let t = today();
        let day = |n: i64, time: &str| format!("{} {time}", add_days(&t, -n));

        // ---- staff
        for (name, role, pin) in [
            ("Kamran", Role::Manager, "2222"),
            ("Faisal", Role::Sales, "3333"),
            ("Hina", Role::Cashier, "4444"),
            ("Nadia", Role::Advisor, "5555"),
            ("Imran", Role::Technician, ""),
            ("Zubair", Role::Technician, ""),
        ] {
            self.save_user(UserInput {
                id: None,
                name: name.into(),
                role: Some(role),
                phone: String::new(),
                pin: if pin.is_empty() { None } else { Some(pin.into()) },
                active: true,
            })?;
        }
        let techs = self.technicians()?;
        let imran = techs.iter().find(|u| u.name == "Imran").map(|u| u.id);
        let zubair = techs.iter().find(|u| u.name == "Zubair").map(|u| u.id);

        // ---- categories
        let mut cat = std::collections::HashMap::new();
        for (name, prefix) in [
            ("Batteries", "BAT"),
            ("Chargers", "CHG"),
            ("Brakes", "BRK"),
            ("Tyres", "TYR"),
            ("Electrical", "ELC"),
            ("Body Parts", "BDY"),
            ("Accessories", "ACC"),
        ] {
            cat.insert(prefix, self.save_category(None, name, prefix)?);
        }

        // ---- suppliers
        let sup1 = self.save_supplier(Supplier {
            name: "Power Cells Traders".into(),
            phone: "0300-5550101".into(),
            ..Default::default()
        })?;
        self.save_supplier(Supplier { name: "City Auto Parts".into(), phone: "0321-5550202".into(), ..Default::default() })?;

        // ---- products: (code, name, kind, cost, price, stock, low, warranty months, fits)
        let products: &[(&str, &str, ProductKind, i64, i64, i64, i64, i64, &str)] = &[
            ("BAT-001", "Lithium Battery 60V 30Ah", ProductKind::Part, 85_000, 105_000, 4, 2, 24, "Spark S1, Breeze"),
            ("BAT-002", "Lithium Battery 72V 40Ah", ProductKind::Part, 120_000, 148_000, 2, 2, 24, "Spark X2 Pro"),
            ("CHG-001", "Fast Charger 60V 5A", ProductKind::Part, 9_500, 13_500, 7, 3, 6, "Spark S1, Breeze"),
            ("CHG-002", "Charger 72V 4A", ProductKind::Part, 11_000, 15_500, 3, 3, 6, "Spark X2 Pro"),
            ("BRK-001", "Front Brake Pads", ProductKind::Part, 900, 1_600, 30, 6, 0, "All models"),
            ("BRK-002", "Rear Brake Shoe", ProductKind::Part, 700, 1_300, 8, 6, 0, "All models"),
            ("BRK-003", "Disc Rotor 220mm", ProductKind::Part, 2_800, 4_500, 7, 3, 0, "Spark X2 Pro"),
            ("TYR-001", "Tubeless Tyre 90/90-12", ProductKind::Part, 4_200, 6_000, 12, 4, 0, "Spark S1, Spark X2 Pro"),
            ("TYR-002", "Tubeless Tyre 100/90-10", ProductKind::Part, 4_600, 6_500, 3, 4, 0, "Breeze"),
            ("ELC-001", "Motor Controller 60V", ProductKind::Part, 14_000, 19_500, 2, 2, 6, "Spark S1, Breeze"),
            ("ELC-002", "Throttle Assembly", ProductKind::Part, 1_800, 3_000, 9, 3, 3, "All models"),
            ("ELC-003", "LED Headlight Unit", ProductKind::Part, 2_400, 3_800, 5, 3, 6, "All models"),
            ("ELC-004", "DC-DC Converter", ProductKind::Part, 3_200, 4_800, 0, 2, 6, "All models"),
            ("ELC-005", "Hub Motor 1500W", ProductKind::Part, 38_000, 48_000, 1, 1, 12, "Spark S1"),
            ("BDY-001", "Side Mirror (pair)", ProductKind::Part, 650, 1_200, 20, 5, 0, "All models"),
            ("BDY-002", "Front Mudguard", ProductKind::Part, 1_500, 2_500, 4, 2, 0, "Spark S1"),
            ("ACC-001", "Full Face Helmet", ProductKind::Accessory, 2_800, 4_500, 16, 4, 0, ""),
            ("ACC-002", "Bike Cover", ProductKind::Accessory, 900, 1_800, 22, 5, 0, ""),
            ("ACC-003", "Phone Holder", ProductKind::Accessory, 450, 1_000, 4, 5, 0, ""),
            ("ACC-004", "Anti-theft Alarm", ProductKind::Accessory, 2_200, 3_500, 6, 2, 6, ""),
            ("ACC-005", "Floor Mat", ProductKind::Accessory, 350, 800, 0, 3, 0, ""),
        ];
        let mut pid = std::collections::HashMap::new();
        for (code, name, kind, cost, price, stock, low, warranty, fits) in products {
            let id = self.save_product(ProductInput {
                id: None,
                code: (*code).into(),
                name: (*name).into(),
                category_id: cat.get(&code[..3]).copied(),
                kind: *kind,
                cost: cost * RS,
                price: price * RS,
                stock: *stock,
                low_stock: *low,
                warranty_months: *warranty,
                serial_tracked: code.starts_with("BAT"),
                fits_models: (*fits).into(),
                location: format!("Rack {}", &code[..1]),
                ..Default::default()
            })?;
            pid.insert(*code, id);
        }
        // A delivery received last week (raises stock and updates the average cost).
        self.receive_stock_at(
            ReceiveInput {
                supplier_id: Some(sup1),
                items: vec![
                    ReceiveLine { product_id: pid["BRK-001"], qty: 10, unit_cost: 950 * RS },
                    ReceiveLine { product_id: pid["ACC-002"], qty: 6, unit_cost: 900 * RS },
                ],
                notes: "Monthly order".into(),
            },
            &day(6, "10:15:00"),
        )?;

        // ---- workshop services
        let mut sid = std::collections::HashMap::new();
        for (code, name, price) in [
            ("SRV-01", "Periodic Service", 1_500),
            ("SRV-02", "Brake Adjustment", 500),
            ("SRV-03", "Battery Health Check", 800),
            ("SRV-04", "Motor / Controller Repair", 3_000),
            ("SRV-05", "Tyre Fitting", 400),
            ("SRV-06", "Wiring Diagnosis", 1_200),
        ] {
            let id = self.save_service(LaborService {
                id: 0,
                code: code.into(),
                name: name.into(),
                price: price * RS,
                active: true,
            })?;
            sid.insert(code, id);
        }

        // ---- vehicle models
        let m1 = self.save_vehicle_model(VehicleModel {
            name: "Spark S1".into(),
            brand: "Spark".into(),
            price: 285_000 * RS,
            cost: 240_000 * RS,
            vehicle_warranty_months: 12,
            battery_warranty_months: 24,
            motor_warranty_months: 24,
            warranty_km: 20_000,
            free_services: 3,
            active: true,
            ..Default::default()
        })?;
        let m2 = self.save_vehicle_model(VehicleModel {
            name: "Spark X2 Pro".into(),
            brand: "Spark".into(),
            price: 365_000 * RS,
            cost: 310_000 * RS,
            vehicle_warranty_months: 12,
            battery_warranty_months: 36,
            motor_warranty_months: 24,
            warranty_km: 30_000,
            free_services: 3,
            active: true,
            ..Default::default()
        })?;
        let m3 = self.save_vehicle_model(VehicleModel {
            name: "Breeze Scooter".into(),
            brand: "Breeze".into(),
            price: 199_000 * RS,
            cost: 165_000 * RS,
            vehicle_warranty_months: 12,
            battery_warranty_months: 12,
            motor_warranty_months: 12,
            warranty_km: 15_000,
            free_services: 2,
            active: true,
            ..Default::default()
        })?;

        // ---- vehicles in stock: (model, chassis, motor, battery, colour, status)
        let units = [
            (m1, "SPK1-26-001041", "MT60-88123", "BT60-55120", "Pearl White", VehicleStatus::InStock),
            (m1, "SPK1-26-001042", "MT60-88124", "BT60-55121", "Matte Black", VehicleStatus::InStock),
            (m1, "SPK1-26-001043", "MT60-88125", "BT60-55122", "Racing Red", VehicleStatus::InStock),
            (m1, "SPK1-26-001044", "MT60-88126", "BT60-55123", "Pearl White", VehicleStatus::InStock),
            (m2, "SPX2-26-000310", "MT72-41001", "BT72-90011", "Graphite Grey", VehicleStatus::InStock),
            (m2, "SPX2-26-000311", "MT72-41002", "BT72-90012", "Ocean Blue", VehicleStatus::Demo),
            (m2, "SPX2-26-000312", "MT72-41003", "BT72-90013", "Matte Black", VehicleStatus::InTransit),
            (m3, "BRZ-26-007701", "MT48-20031", "BT48-33101", "Mint Green", VehicleStatus::InStock),
            (m3, "BRZ-26-007702", "MT48-20032", "BT48-33102", "Sky Blue", VehicleStatus::InStock),
        ];
        let mut vid = Vec::new();
        for (m, chassis, motor, battery, color, status) in units {
            let model = self.get_vehicle_model(m)?;
            vid.push(self.save_vehicle(VehicleInput {
                model_id: Some(m),
                chassis_no: chassis.into(),
                motor_no: motor.into(),
                battery_serial: battery.into(),
                color: color.into(),
                year: 2026,
                cost: model.cost,
                price: model.price,
                status,
                ..Default::default()
            })?);
        }

        // ---- customers
        let mut cust = Vec::new();
        for (name, phone, id_no, address) in [
            ("Ali Hassan", "0300-1112233", "35202-1234567-1", "House 12, Block C, Model Town"),
            ("Sara Malik", "0321-4445566", "35201-7654321-2", "Flat 4, Garden Heights"),
            ("Usman Tariq", "0333-7778899", "", "Main Boulevard, Shop 9"),
            ("Ayesha Noor", "0345-2223344", "", ""),
            ("Bilal Ahmed", "0301-9990011", "", "Street 5, Johar Town"),
            ("Hamza Qureshi", "0312-6667788", "", ""),
        ] {
            cust.push(self.save_customer(CustomerInput {
                id: None,
                name: name.into(),
                phone: phone.into(),
                id_no: id_no.into(),
                address: address.into(),
                ..Default::default()
            })?);
        }

        let line = |code: &str, qty: i64| -> Result<CartLine> {
            let p = self.get_product(pid[code])?;
            Ok(CartLine {
                item_type: ItemType::Product,
                ref_id: Some(p.id),
                code: p.code.clone(),
                description: p.name.clone(),
                qty,
                unit_price: p.price,
                line_discount: 0,
                serial_no: String::new(),
                free: false,
            })
        };
        let sell = |ts: String, items: Vec<CartLine>, customer: Option<i64>, pay: Vec<PaymentInput>, disc: i64| -> Result<SaleReceipt> {
            let total = self.totals(&items, disc).total;
            // An amount of 0 means "the full bill" (keeps the demo correct whatever tax rate is set).
            let pay = if pay.is_empty() {
                vec![PaymentInput::cash(total)]
            } else {
                pay.into_iter().map(|mut p| { if p.amount == 0 { p.amount = total; } p }).collect()
            };
            let tx = self.conn.unchecked_transaction()?;
            let r = self.checkout_inner(
                &CheckoutInput {
                    kind: SaleKind::Counter,
                    customer_id: customer,
                    salesperson_id: None,
                    items,
                    discount: disc,
                    payments: pay,
                    notes: String::new(),
                    booking_id: None,
                },
                SaleOpts { ts: Some(ts), ..Default::default() },
            )?;
            tx.commit()?;
            Ok(r)
        };

        // ---- counter sales over the last 9 days
        sell(day(9, "11:05:00"), vec![line("BRK-001", 2)?, line("BDY-001", 1)?], None, vec![], 0)?;
        sell(day(8, "13:40:00"), vec![line("ACC-001", 1)?, line("ACC-002", 1)?], None, vec![], 0)?;
        sell(day(8, "17:20:00"), vec![line("TYR-001", 1)?], Some(cust[2]), vec![], 0)?;
        sell(day(7, "10:30:00"), vec![line("ELC-002", 1)?, line("BRK-001", 1)?], None, vec![], 0)?;
        sell(day(6, "12:10:00"), vec![line("CHG-001", 1)?], Some(cust[3]), vec![PaymentInput::bank(13_500 * RS, "Meezan Bank", "TRX88123")], 0)?;
        sell(day(5, "15:45:00"), vec![line("ACC-001", 2)?, line("ACC-004", 1)?], Some(cust[4]), vec![], 500 * RS)?;
        sell(day(4, "11:25:00"), vec![line("BDY-001", 2)?, line("ACC-002", 2)?], None, vec![], 0)?;
        sell(day(3, "16:00:00"), vec![line("BRK-002", 2)?, line("BRK-001", 2)?], None, vec![], 0)?;
        sell(day(2, "12:35:00"), vec![line("TYR-001", 2)?], Some(cust[5]), vec![PaymentInput::cash(8_000 * RS)], 0)?;
        sell(day(1, "18:10:00"), vec![line("ACC-003", 1)?, line("ACC-002", 1)?], None, vec![], 0)?;
        sell(day(0, "10:20:00"), vec![line("BRK-001", 1)?, line("ELC-003", 1)?], None, vec![], 0)?;
        sell(day(0, "12:50:00"), vec![line("ACC-001", 1)?], None, vec![PaymentInput::bank(0, "HBL", "IBFT-55021")], 0)?;

        // ---- vehicle sales (warranties and free-service coupons are created automatically)
        let sell_bike = |ts: String, v: i64, customer: i64, extra: Vec<CartLine>, pay: Vec<PaymentInput>| -> Result<SaleReceipt> {
            let bike = self.get_vehicle(v)?;
            let mut items = vec![CartLine {
                item_type: ItemType::Vehicle,
                ref_id: Some(bike.id),
                code: bike.chassis_no.clone(),
                description: format!("{} — {}", bike.model_name, bike.color),
                qty: 1,
                unit_price: bike.price,
                line_discount: 0,
                serial_no: String::new(),
                free: false,
            }];
            items.extend(extra);
            let tx = self.conn.unchecked_transaction()?;
            let r = self.checkout_inner(
                &CheckoutInput {
                    kind: SaleKind::Vehicle,
                    customer_id: Some(customer),
                    salesperson_id: None,
                    items,
                    discount: 0,
                    payments: pay,
                    notes: String::new(),
                    booking_id: None,
                },
                SaleOpts { ts: Some(ts), ..Default::default() },
            )?;
            tx.commit()?;
            Ok(r)
        };
        sell_bike(
            day(8, "15:00:00"),
            vid[3],
            cust[0],
            vec![line("ACC-001", 1)?],
            vec![PaymentInput::bank(289_500 * RS, "Meezan Bank", "TRX77001")],
        )?;
        sell_bike(day(3, "14:20:00"), vid[8], cust[1], vec![], vec![
            PaymentInput::cash(100_000 * RS),
            PaymentInput::bank(80_000 * RS, "UBL", "IBFT-99114"),
        ])?;
        self.set_registration(vid[3], "LEB-26-4471")?;

        // ---- booking with advance on a specific bike
        self.create_booking_at(
            BookingInput {
                customer_id: cust[4],
                model_id: Some(m2),
                vehicle_id: Some(vid[4]),
                color: "Graphite Grey".into(),
                agreed_price: 360_000 * RS,
                expected_date: add_days(&t, 4),
                notes: "Wants number plate done by shop".into(),
                advance: vec![PaymentInput::cash(50_000 * RS)],
            },
            &day(2, "11:00:00"),
        )?;

        // ---- workshop
        // Customer's own bike (bought elsewhere)
        let ext = self.add_customer_bike(cust[2], "Other EV 60V", "", "LEC-25-1903", "Black")?;

        // 1) Free first service on Ali's new bike, delivered
        let j1 = self.create_job_at(
            JobInput {
                vehicle_id: vid[3],
                customer_id: None,
                odometer: 512,
                battery_note: "96% health".into(),
                complaint: "First free service".into(),
                technician_id: imran,
                is_warranty: false,
                use_coupon: true,
            },
            &day(1, "10:00:00"),
        )?;
        self.add_job_service(j1, Some(sid["SRV-01"]), "", 0, false)?;
        self.add_job_part(j1, pid["BRK-001"], 1, false)?;
        self.deliver_job_at(j1, 0, vec![PaymentInput::cash(1_600 * RS)], "", 3_000, &day(1, "16:30:00"))?;

        // 2) In progress
        let j2 = self.create_job_at(
            JobInput {
                vehicle_id: ext,
                customer_id: Some(cust[2]),
                odometer: 8_420,
                battery_note: "Range dropped to 45 km".into(),
                complaint: "Bike cuts off at full throttle, check controller and battery".into(),
                technician_id: zubair,
                ..Default::default()
            },
            &day(0, "09:40:00"),
        )?;
        self.add_job_service(j2, Some(sid["SRV-06"]), "", 0, false)?;
        self.add_job_service(j2, Some(sid["SRV-03"]), "", 0, false)?;
        self.add_job_part(j2, pid["ELC-002"], 1, false)?;
        self.set_job_status(j2, JobStatus::InProgress)?;

        // 3) Ready for pick-up
        let ext2 = self.add_customer_bike(cust[5], "Breeze Scooter", "BRZ-25-004410", "", "White")?;
        let j3 = self.create_job_at(
            JobInput {
                vehicle_id: ext2,
                customer_id: Some(cust[5]),
                odometer: 3_110,
                complaint: "Rear tyre puncture, brakes squeaking".into(),
                technician_id: imran,
                ..Default::default()
            },
            &day(0, "11:15:00"),
        )?;
        self.add_job_service(j3, Some(sid["SRV-05"]), "", 0, false)?;
        self.add_job_service(j3, Some(sid["SRV-02"]), "", 0, false)?;
        self.add_job_part(j3, pid["TYR-002"], 1, false)?;
        self.set_job_status(j3, JobStatus::Ready)?;

        // 4) Waiting for parts
        let ext3 = self.add_customer_bike(cust[3], "Spark S1", "SPK1-25-000877", "LEA-25-7712", "Red")?;
        let j4 = self.create_job_at(
            JobInput {
                vehicle_id: ext3,
                customer_id: Some(cust[3]),
                odometer: 14_900,
                battery_note: "OK".into(),
                complaint: "DC-DC converter faulty, lights flicker".into(),
                technician_id: zubair,
                is_warranty: true,
                ..Default::default()
            },
            &day(1, "14:00:00"),
        )?;
        self.add_job_service(j4, Some(sid["SRV-06"]), "", 0, false)?;
        self.set_job_status(j4, JobStatus::WaitingParts)?;

        // 5) Just received
        self.create_job_at(
            JobInput {
                vehicle_id: vid[8],
                customer_id: None,
                odometer: 240,
                complaint: "Customer reports a rattle from the rear".into(),
                use_coupon: true,
                ..Default::default()
            },
            &day(0, "13:05:00"),
        )?;

        // An older delivered job whose next service is due in a few days.
        let ext4 = self.add_customer_bike(cust[1], "Spark X2 Pro", "SPX2-25-000155", "LEF-25-3301", "Blue")?;
        let j6 = self.create_job_at(
            JobInput {
                vehicle_id: ext4,
                customer_id: Some(cust[1]),
                odometer: 5_100,
                complaint: "General service".into(),
                technician_id: imran,
                ..Default::default()
            },
            &day(30, "10:00:00"),
        )?;
        self.add_job_service(j6, Some(sid["SRV-01"]), "", 0, false)?;
        self.deliver_job_at(j6, 0, vec![PaymentInput::cash(1_500 * RS)], &add_days(&t, 3), 8_000, &day(29, "12:00:00"))?;
        Ok(())
    }
}
