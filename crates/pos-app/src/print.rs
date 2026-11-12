//! Printable receipts, invoices and job cards.
//!
//! Documents are rendered as HTML and opened in the default browser, which shows the
//! system print dialog. This works with every printer installed on macOS or Windows
//! (thermal receipt printers included) and can also "Save as PDF".

use pos_core::util::{pretty_date, pretty_ts};
use pos_core::{ItemType, JobCard, Pos, SaleDetail, SaleKind};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Paper {
    /// 80 mm thermal roll.
    Thermal,
    A4,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

const BASE_CSS: &str = r#"
*{box-sizing:border-box;margin:0;padding:0}
body{font-family:-apple-system,"Segoe UI",Inter,Roboto,Arial,sans-serif;color:#0f172a;background:#eef2f7}
.page{background:#fff;margin:24px auto;box-shadow:0 10px 30px rgba(15,23,42,.12)}
.muted{color:#64748b}.right{text-align:right}.center{text-align:center}.bold{font-weight:700}
table{width:100%;border-collapse:collapse}
@media print{body{background:#fff}.page{margin:0;box-shadow:none}.noprint{display:none}}
.bar{position:sticky;top:0;background:#0f172a;color:#fff;padding:10px 16px;display:flex;gap:10px;align-items:center;font-size:14px}
.bar button{background:#14b8a6;color:#fff;border:0;border-radius:8px;padding:8px 14px;font-weight:700;cursor:pointer;font-size:14px}
"#;

const THERMAL_CSS: &str = r#"
@page{size:80mm auto;margin:3mm}
.page{width:76mm;padding:4mm 3mm;font-size:11.5px;line-height:1.35}
h1{font-size:16px;text-align:center}
.hr{border-top:1px dashed #94a3b8;margin:6px 0}
td{padding:2px 0;vertical-align:top}
.big{font-size:15px;font-weight:800}
"#;

const A4_CSS: &str = r#"
@page{size:A4;margin:14mm}
.page{width:210mm;min-height:297mm;padding:16mm 16mm;font-size:12.5px;line-height:1.45}
@media print{.page{width:auto;min-height:0;padding:0}}
.head{display:flex;justify-content:space-between;align-items:flex-start;border-bottom:3px solid #0d9488;padding-bottom:14px;margin-bottom:18px}
.brand{display:flex;gap:12px;align-items:center}
.logo{width:44px;height:44px;border-radius:12px;background:linear-gradient(135deg,#2dd4bf,#0d9488);color:#fff;font-size:24px;display:flex;align-items:center;justify-content:center;font-weight:900}
.shop{font-size:22px;font-weight:800}
.doc{font-size:24px;font-weight:800;color:#0d9488;text-align:right}
.boxes{display:flex;gap:14px;margin-bottom:18px}
.box{flex:1;background:#f8fafc;border:1px solid #e2e8f0;border-radius:10px;padding:12px 14px}
.box h4{font-size:10.5px;letter-spacing:.06em;color:#64748b;margin-bottom:4px}
th{font-size:10.5px;letter-spacing:.05em;color:#64748b;text-align:left;border-bottom:1px solid #cbd5e1;padding:8px 6px}
td{padding:9px 6px;border-bottom:1px solid #eef2f7;vertical-align:top}
.totals{width:300px;margin-left:auto;margin-top:12px}
.totals td{border:0;padding:4px 6px}
.grand td{font-size:16px;font-weight:800;border-top:2px solid #0f172a;padding-top:8px}
.warranty{margin-top:18px;border:1px solid #99f6e4;background:#f0fdfa;border-radius:10px;padding:12px 14px}
.warranty h3{color:#0f766e;font-size:13px;margin-bottom:6px}
.terms{margin-top:22px;font-size:11px;color:#475569;white-space:pre-wrap}
.sign{display:flex;justify-content:space-between;margin-top:46px;font-size:11px;color:#475569}
.sign div{border-top:1px solid #94a3b8;padding-top:6px;width:45%}
.foot{margin-top:24px;text-align:center;color:#64748b;font-size:11px}
"#;

fn shell(title: &str, css: &str, body: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>{}</title><style>{}{}</style></head>
<body><div class="bar noprint"><button onclick="window.print()">Print</button><span>Use the print dialog to choose your printer or “Save as PDF”.</span></div>
{}<script>window.addEventListener('load',()=>setTimeout(()=>window.print(),300));</script></body></html>"#,
        esc(title),
        BASE_CSS,
        css,
        body
    )
}

fn doc_title(sale: &SaleDetail) -> &'static str {
    match sale.sale.kind {
        SaleKind::Return => "Refund Receipt",
        SaleKind::Vehicle => "Sales Invoice",
        SaleKind::Workshop => "Workshop Invoice",
        SaleKind::Counter => "Receipt",
    }
}

pub fn sale_html(pos: &Pos, sale: &SaleDetail, paper: Paper) -> String {
    let s = pos.settings();
    let m = pos.money();
    let money = |v: i64| esc(&m.fmt(v));
    let title = format!("{} {}", doc_title(sale), sale.sale.invoice_no);
    let balance = sale.sale.balance();

    // Vehicle details for bike invoices.
    let mut vehicle_html = String::new();
    for it in sale.items.iter().filter(|i| i.item_type == ItemType::Vehicle) {
        if let Some(vid) = it.vehicle_id {
            if let Ok(v) = pos.get_vehicle(vid) {
                let mut w = Vec::new();
                if !v.vehicle_warranty_until.is_empty() {
                    w.push(format!("Vehicle until {}", pretty_date(&v.vehicle_warranty_until)));
                }
                if !v.battery_warranty_until.is_empty() {
                    w.push(format!("Battery until {}", pretty_date(&v.battery_warranty_until)));
                }
                if !v.motor_warranty_until.is_empty() {
                    w.push(format!("Motor until {}", pretty_date(&v.motor_warranty_until)));
                }
                if v.warranty_km > 0 {
                    w.push(format!("or {} km", v.warranty_km));
                }
                let coupons = pos.vehicle_coupons(v.id).map(|c| c.len()).unwrap_or(0);
                vehicle_html.push_str(&format!(
                    "<div class=\"warranty\"><h3>Vehicle & warranty</h3><div><b>{}</b>{} · Chassis <b>{}</b> · Motor {} · Battery {}{}</div><div>{}</div>{}</div>",
                    esc(&v.model_name),
                    if v.color.is_empty() { String::new() } else { format!(" ({})", esc(&v.color)) },
                    esc(&v.chassis_no),
                    esc(&v.motor_no),
                    esc(&v.battery_serial),
                    if v.registration_no.is_empty() { String::new() } else { format!(" · Reg. {}", esc(&v.registration_no)) },
                    esc(&w.join(" · ")),
                    if coupons > 0 { format!("<div>{coupons} free service coupons included.</div>") } else { String::new() }
                ));
            }
        }
    }

    match paper {
        Paper::Thermal => {
            let mut b = String::from("<div class=\"page\">");
            b.push_str(&format!("<h1>{}</h1>", esc(&s.shop_name)));
            for line in [&s.shop_address, &s.shop_phone] {
                if !line.is_empty() {
                    b.push_str(&format!("<div class=\"center muted\">{}</div>", esc(line)));
                }
            }
            if !s.tax_number.is_empty() {
                b.push_str(&format!("<div class=\"center muted\">Tax no. {}</div>", esc(&s.tax_number)));
            }
            b.push_str("<div class=\"hr\"></div>");
            b.push_str(&format!(
                "<div class=\"center bold\">{}</div><div class=\"center\">{} · {}</div>",
                doc_title(sale).to_uppercase(),
                esc(&sale.sale.invoice_no),
                esc(&pretty_ts(&sale.sale.created_at))
            ));
            if !sale.sale.customer_name.is_empty() {
                b.push_str(&format!(
                    "<div class=\"center\">Customer: {} {}</div>",
                    esc(&sale.sale.customer_name),
                    esc(&sale.customer_phone)
                ));
            }
            b.push_str("<div class=\"hr\"></div><table>");
            for it in &sale.items {
                b.push_str(&format!(
                    "<tr><td colspan=2>{}<br><span class=muted>{} × {}{}</span></td><td class=right>{}</td></tr>",
                    esc(&it.description),
                    it.qty.abs(),
                    money(it.unit_price),
                    if it.code.is_empty() { String::new() } else { format!(" · {}", esc(&it.code)) },
                    if it.free { "FREE".into() } else { money(it.unit_price * it.qty - it.line_discount) }
                ));
            }
            b.push_str("</table><div class=\"hr\"></div><table>");
            b.push_str(&format!("<tr><td>Subtotal</td><td class=right>{}</td></tr>", money(sale.subtotal)));
            if sale.discount != 0 {
                b.push_str(&format!("<tr><td>Discount</td><td class=right>− {}</td></tr>", money(sale.discount)));
            }
            if sale.tax != 0 {
                b.push_str(&format!("<tr><td>{}</td><td class=right>{}</td></tr>", esc(&s.tax_label), money(sale.tax)));
            }
            b.push_str(&format!("<tr class=big><td>TOTAL</td><td class=right>{}</td></tr>", money(sale.sale.total)));
            for p in &sale.payments {
                b.push_str(&format!("<tr><td>{}</td><td class=right>{}</td></tr>", esc(p.method.label()), money(p.amount)));
            }
            if balance > 0 {
                b.push_str(&format!("<tr class=bold><td>Balance due</td><td class=right>{}</td></tr>", money(balance)));
            }
            b.push_str("</table><div class=\"hr\"></div>");
            if !sale.salesperson_name.is_empty() {
                b.push_str(&format!("<div class=center muted>Served by {}</div>", esc(&sale.salesperson_name)));
            }
            b.push_str(&format!("<div class=\"center bold\" style=\"margin-top:6px\">{}</div></div>", esc(&s.receipt_footer)));
            shell(&title, THERMAL_CSS, &b)
        }
        Paper::A4 => {
            let mut b = String::from("<div class=\"page\">");
            b.push_str(&format!(
                "<div class=head><div class=brand><div class=logo>⚡</div><div><div class=shop>{}</div><div class=muted>{}</div><div class=muted>{}{}</div></div></div>\
                 <div><div class=doc>{}</div><div class=right><b>{}</b></div><div class='right muted'>{}</div></div></div>",
                esc(&s.shop_name),
                esc(&s.shop_address),
                esc(&s.shop_phone),
                if s.tax_number.is_empty() { String::new() } else { format!(" · Tax no. {}", esc(&s.tax_number)) },
                doc_title(sale),
                esc(&sale.sale.invoice_no),
                esc(&pretty_ts(&sale.sale.created_at))
            ));
            b.push_str("<div class=boxes>");
            b.push_str(&format!(
                "<div class=box><h4>BILL TO</h4><div class=bold>{}</div><div>{}</div></div>",
                if sale.sale.customer_name.is_empty() { "Walk-in customer".into() } else { esc(&sale.sale.customer_name) },
                esc(&sale.customer_phone)
            ));
            b.push_str(&format!(
                "<div class=box><h4>DETAILS</h4><div>Served by {}</div>{}{}</div>",
                esc(&sale.salesperson_name),
                if sale.notes.is_empty() { String::new() } else { format!("<div>{}</div>", esc(&sale.notes)) },
                if sale.original_invoice.is_empty() { String::new() } else { format!("<div>Refund for {}</div>", esc(&sale.original_invoice)) }
            ));
            b.push_str("</div><table><tr><th>#</th><th>ITEM</th><th>CODE</th><th class=right>QTY</th><th class=right>PRICE</th><th class=right>AMOUNT</th></tr>");
            for (i, it) in sale.items.iter().enumerate() {
                let mut extra = Vec::new();
                if it.line_discount > 0 {
                    extra.push(format!("discount {}", m.fmt(it.line_discount)));
                }
                if !it.warranty_until.is_empty() {
                    extra.push(format!("warranty until {}", pretty_date(&it.warranty_until)));
                }
                if it.free {
                    extra.push("free of charge (warranty / coupon)".into());
                }
                b.push_str(&format!(
                    "<tr><td>{}</td><td><b>{}</b>{}</td><td class=muted>{}</td><td class=right>{}</td><td class=right>{}</td><td class=right><b>{}</b></td></tr>",
                    i + 1,
                    esc(&it.description),
                    if extra.is_empty() { String::new() } else { format!("<br><span class=muted>{}</span>", esc(&extra.join(" · "))) },
                    esc(&it.code),
                    it.qty.abs(),
                    money(it.unit_price),
                    if it.free { "FREE".into() } else { money(it.unit_price * it.qty - it.line_discount) }
                ));
            }
            b.push_str("</table><table class=totals>");
            b.push_str(&format!("<tr><td>Subtotal</td><td class=right>{}</td></tr>", money(sale.subtotal)));
            if sale.discount != 0 {
                b.push_str(&format!("<tr><td>Discount</td><td class=right>− {}</td></tr>", money(sale.discount)));
            }
            if sale.tax != 0 {
                b.push_str(&format!(
                    "<tr><td>{} {}{}</td><td class=right>{}</td></tr>",
                    esc(&s.tax_label),
                    pos_core::money::fmt_percent(s.tax_rate_bp),
                    if s.tax_inclusive { " (incl.)" } else { "" },
                    money(sale.tax)
                ));
            }
            b.push_str(&format!("<tr class=grand><td>Total</td><td class=right>{}</td></tr>", money(sale.sale.total)));
            for p in &sale.payments {
                b.push_str(&format!(
                    "<tr><td class=muted>{} {} · {}</td><td class=right>{}</td></tr>",
                    esc(p.method.label()),
                    esc(&p.bank_name),
                    esc(&pretty_date(&p.created_at)),
                    money(p.amount)
                ));
            }
            b.push_str(&format!(
                "<tr class=bold><td>{}</td><td class=right>{}</td></tr></table>",
                if balance > 0 { "Balance due" } else { "Paid" },
                if balance > 0 { money(balance) } else { money(sale.sale.paid) }
            ));
            b.push_str(&vehicle_html);
            if !s.invoice_terms.is_empty() && sale.sale.kind != SaleKind::Counter {
                b.push_str(&format!("<div class=terms><b>Terms</b>\n{}</div>", esc(&s.invoice_terms)));
            }
            if sale.sale.kind == SaleKind::Vehicle {
                b.push_str("<div class=sign><div>Customer signature</div><div>For the shop</div></div>");
            }
            b.push_str(&format!("<div class=foot>{}</div></div>", esc(&s.receipt_footer)));
            shell(&title, A4_CSS, &b)
        }
    }
}

pub fn job_card_html(pos: &Pos, job: &JobCard) -> String {
    let s = pos.settings();
    let m = pos.money();
    let money = |v: i64| esc(&m.fmt(v));
    let mut b = String::from("<div class=\"page\">");
    b.push_str(&format!(
        "<div class=head><div class=brand><div class=logo>⚡</div><div><div class=shop>{}</div><div class=muted>{}</div><div class=muted>{}</div></div></div>\
         <div><div class=doc>Job Card</div><div class=right><b>{}</b></div><div class='right muted'>Opened {}</div><div class='right muted'>Status: {}</div></div></div>",
        esc(&s.shop_name),
        esc(&s.shop_address),
        esc(&s.shop_phone),
        esc(&job.job_no),
        esc(&pretty_ts(&job.created_at)),
        esc(job.status.label())
    ));
    b.push_str(&format!(
        "<div class=boxes><div class=box><h4>CUSTOMER</h4><div class=bold>{}</div><div>{}</div></div>\
         <div class=box><h4>BIKE</h4><div class=bold>{}</div><div>Chassis {} · Odometer {} km</div><div>Battery: {}</div></div>\
         <div class=box><h4>TECHNICIAN</h4><div class=bold>{}</div>{}</div></div>",
        esc(&job.customer_name),
        esc(&job.customer_phone),
        esc(&job.vehicle_label),
        esc(&job.chassis_no),
        job.odometer,
        esc(&job.battery_note),
        if job.technician_name.is_empty() { "Not assigned".into() } else { esc(&job.technician_name) },
        if job.is_warranty { "<div><b>Warranty job</b></div>".to_string() } else if job.coupon_seq > 0 { format!("<div>Free service coupon #{}</div>", job.coupon_seq) } else { String::new() }
    ));
    b.push_str(&format!(
        "<div class=box style='margin-bottom:14px'><h4>COMPLAINT / WORK NEEDED</h4><div>{}</div></div>",
        esc(&job.complaint)
    ));
    if !job.diagnosis.is_empty() {
        b.push_str(&format!(
            "<div class=box style='margin-bottom:14px'><h4>FINDINGS</h4><div>{}</div></div>",
            esc(&job.diagnosis)
        ));
    }
    b.push_str("<table><tr><th>TYPE</th><th>DESCRIPTION</th><th class=right>QTY</th><th class=right>PRICE</th><th class=right>AMOUNT</th></tr>");
    for it in &job.items {
        b.push_str(&format!(
            "<tr><td>{}</td><td><b>{}</b> <span class=muted>{}</span></td><td class=right>{}</td><td class=right>{}</td><td class=right>{}</td></tr>",
            it.kind.label(),
            esc(&it.description),
            esc(&it.code),
            it.qty,
            money(it.unit_price),
            if it.free { "FREE".into() } else { money(it.charge()) }
        ));
    }
    if job.items.is_empty() {
        b.push_str("<tr><td colspan=5 class=muted>No parts or labour yet.</td></tr>");
    }
    b.push_str(&format!(
        "</table><table class=totals><tr><td>Parts</td><td class=right>{}</td></tr><tr><td>Labour</td><td class=right>{}</td></tr><tr class=grand><td>Estimate</td><td class=right>{}</td></tr></table>",
        money(job.parts_total),
        money(job.labor_total),
        money(job.total())
    ));
    b.push_str("<div class=sign><div>Customer signature (bike received / delivered)</div><div>Service advisor</div></div></div>");
    shell(&format!("Job card {}", job.job_no), A4_CSS, &b)
}

/// Write the document and open it with the default browser.
pub fn open_html(data_dir: &Path, name: &str, html: &str) -> std::io::Result<PathBuf> {
    let dir = data_dir.join("print");
    std::fs::create_dir_all(&dir)?;
    // Keep the print folder small.
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut files: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        files.sort();
        while files.len() > 50 {
            let _ = std::fs::remove_file(files.remove(0));
        }
    }
    let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    let path = dir.join(format!("{safe}.html"));
    std::fs::write(&path, html)?;
    open_path(&path)?;
    Ok(path)
}

pub fn open_path(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::process::Command::new("xdg-open").arg(path).spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_html() {
        assert_eq!(esc("<b>&\"</b>"), "&lt;b&gt;&amp;&quot;&lt;/b&gt;");
    }

    #[test]
    fn renders_sale_documents() {
        let pos = Pos::open_in_memory().unwrap();
        pos.complete_setup(pos_core::SetupInput {
            settings: pos_core::Settings { shop_name: "Test <Shop>".into(), ..Default::default() },
            owner_name: "Owner".into(),
            owner_pin: "1234".into(),
            demo_data: true,
        })
        .unwrap();
        let sales = pos.list_sales(&pos_core::SalesFilter::default()).unwrap();
        assert!(!sales.is_empty());
        for s in sales.iter().take(5) {
            let d = pos.get_sale(s.id).unwrap();
            let a4 = sale_html(&pos, &d, Paper::A4);
            let th = sale_html(&pos, &d, Paper::Thermal);
            assert!(a4.contains(&d.sale.invoice_no));
            assert!(th.contains(&d.sale.invoice_no));
            assert!(a4.contains("Test &lt;Shop&gt;"));
        }
        let vehicle_sale = sales.iter().find(|s| s.kind == SaleKind::Vehicle).unwrap();
        let html = sale_html(&pos, &pos.get_sale(vehicle_sale.id).unwrap(), Paper::A4);
        assert!(html.contains("Vehicle & warranty"));
        // Developer preview: EV_POS_PRINT_OUT=/some/dir cargo test
        if let Ok(dir) = std::env::var("EV_POS_PRINT_OUT") {
            let dir = std::path::Path::new(&dir);
            std::fs::write(dir.join("invoice.html"), &html).unwrap();
            let counter = sales.iter().find(|s| s.kind == SaleKind::Counter).unwrap();
            let th = sale_html(&pos, &pos.get_sale(counter.id).unwrap(), Paper::Thermal);
            std::fs::write(dir.join("receipt.html"), th).unwrap();
        }
        let jobs = pos.list_jobs(&pos_core::JobFilter::default()).unwrap();
        let job = pos.get_job(jobs[0].id).unwrap();
        assert!(job_card_html(&pos, &job).contains(&job.job_no));
    }
}
